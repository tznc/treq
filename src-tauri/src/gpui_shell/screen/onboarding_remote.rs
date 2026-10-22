//! Dashboard's explicit-endpoint repository form, in its own card above
//! the onboarding card while an SSH endpoint is connected but no repository is open: the
//! cut-off notice with Reauthenticate, or `RemoteRepositorySelector`.
//!
//! The window keeps the form's state and runs probe, open, clone and init: GPUI
//! draws what the route says and sends what is typed and pressed
//! (`remoteRepo.*`).

use crate::gpui_shell::ui::zpx;
use gpui_kit::component::input::{InputEvent, InputState};
use serde::Deserialize;
use tauri_plugin_gpui::gpui::{
  self, div, prelude::*, App, ClickEvent, Context, Div, Entity, FontWeight, SharedString,
  Subscription, Window,
};

use super::{ActionSink, ScreenAction};
use crate::gpui_shell::{
  theme::{tw, Palette},
  ui::{self, Button, Size, TextSize},
};

/// A saved repository on the endpoint (`SavedRemoteRepositoryRecord`).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct SavedRepo {
  pub id: String,
  pub display_name: String,
  pub canonical_remote_path: String,
}

/// What a probe of the path found (`RemoteRepoProbe`).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Probe {
  pub is_repo: bool,
  pub needs_clone: bool,
}

/// The form's state, as Dashboard keeps it.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Form {
  /// The endpoint's cutoff reason (`session_ended`, ...), if cut off.
  pub cutoff: Option<String>,
  /// The selector shows: not cut off, and no repository connected yet.
  pub selector: bool,
  pub saved: Vec<SavedRepo>,
  pub selected_id: Option<String>,
  pub path: String,
  pub probe: Option<Probe>,
  pub clone_url: String,
  pub confirm_init: bool,
  pub busy: bool,
  pub error: Option<String>,
}

/// "Access blocked (session ended)": the reason with spaces.
pub fn cutoff_title(reason: &str) -> String {
  format!(
    "Access blocked ({})",
    reason.split('_').collect::<Vec<_>>().join(" ")
  )
}

/// A text field whose value React owns: typing sends it, and a value React
/// sets on its own (a saved repository picked) replaces what is shown.
struct Field {
  state: Entity<InputState>,
  /// What GPUI last sent, so React's echo of it never resets the caret.
  sent: String,
  _subscription: Subscription,
}

impl Field {
  fn new<V: 'static>(
    name: &'static str,
    value: &str,
    react: ActionSink,
    window: &mut Window,
    cx: &mut Context<V>,
  ) -> Self {
    let state = cx.new(|cx| InputState::new(window, cx).default_value(value.to_string()));
    let subscription = cx.subscribe(&state, move |_, state, event: &InputEvent, cx| {
      if matches!(event, InputEvent::Change) {
        let value = state.read(cx).value().to_string();
        react(
          ScreenAction::Field {
            field: name.into(),
            value: value.into(),
          },
          cx,
        );
        cx.notify();
      }
    });
    Self {
      state,
      sent: value.to_string(),
      _subscription: subscription,
    }
  }

  fn sync(&mut self, value: &str, window: &mut Window, cx: &mut App) {
    let shown = self.state.read(cx).value().to_string();
    if value != shown && value != self.sent {
      self.state.update(cx, |state, cx| {
        state.set_value(value.to_string(), window, cx)
      });
    }
    self.sent = value.to_string();
  }

  fn value(&self, cx: &App) -> String {
    self.state.read(cx).value().to_string()
  }
}

/// The form's inputs, kept across renders.
#[derive(Default)]
pub struct Inputs {
  path: Option<Field>,
  clone_url: Option<Field>,
}

impl Inputs {
  /// Creates the inputs once, and takes React's values into them.
  pub fn sync<V: 'static>(
    &mut self,
    form: &Form,
    react: &ActionSink,
    window: &mut Window,
    cx: &mut Context<V>,
  ) {
    match &mut self.path {
      Some(field) => field.sync(&form.path, window, cx),
      None => {
        self.path = Some(Field::new(
          "remoteRepo.path",
          &form.path,
          react.clone(),
          window,
          cx,
        ))
      }
    }
    match &mut self.clone_url {
      Some(field) => field.sync(&form.clone_url, window, cx),
      None => {
        self.clone_url = Some(Field::new(
          "remoteRepo.cloneUrl",
          &form.clone_url,
          react.clone(),
          window,
          cx,
        ))
      }
    }
  }
}

fn press(react: &ActionSink, id: &'static str) -> impl Fn(&ClickEvent, &mut Window, &mut App) {
  let react = react.clone();
  move |_, _, cx| react(ScreenAction::Press { id: id.into() }, cx)
}

fn field(
  react: &ActionSink,
  name: &'static str,
  value: serde_json::Value,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) {
  let react = react.clone();
  move |_, _, cx| {
    react(
      ScreenAction::Field {
        field: name.into(),
        value: value.clone(),
      },
      cx,
    )
  }
}

/// `rounded-md border border-border/60 bg-background px-2 py-1.5`.
fn input(id: &'static str, state: &Entity<InputState>, p: &Palette) -> impl IntoElement {
  div()
    .id(id)
    .debug_selector(|| id.into())
    .w_full()
    .h(zpx(34.))
    .px_2()
    .flex()
    .items_center()
    .rounded(zpx(8.))
    .border_1()
    .border_color(p.border.opacity(0.6))
    .bg(p.background)
    .child(ui::input(state).appearance(false).bordered(false).flex_1())
}

fn caption(text: &'static str) -> impl IntoElement {
  ui::text(div(), TextSize::Sm)
    .font_weight(FontWeight::MEDIUM)
    .child(text)
}

fn selector(
  form: &Form,
  inputs: &Inputs,
  react: &ActionSink,
  dark: bool,
  p: &Palette,
  cx: &App,
) -> Div {
  let path = inputs
    .path
    .as_ref()
    .map(|f| f.value(cx))
    .unwrap_or_default();
  let clone_url = inputs
    .clone_url
    .as_ref()
    .map(|f| f.value(cx))
    .unwrap_or_default();
  let saved = (!form.saved.is_empty()).then(|| {
    div()
      .flex()
      .flex_col()
      .gap_1()
      .child(caption("Saved repositories"))
      .child(
        div()
          .flex()
          .flex_col()
          .gap_1()
          .children(form.saved.iter().map(|repo| {
            let selected = form.selected_id.as_deref() == Some(repo.id.as_str());
            ui::text(div(), TextSize::Sm)
              .id(SharedString::from(format!("remote-saved-repo-{}", repo.id)))
              .debug_selector(|| "remote-saved-repo".into())
              .w_full()
              .rounded(zpx(8.))
              .border_1()
              .px_2()
              .py(zpx(6.))
              .cursor_pointer()
              .map(|d| {
                if selected {
                  d.border_color(p.primary).bg(p.muted.opacity(0.6))
                } else {
                  d.border_color(p.border.opacity(0.6))
                }
              })
              .on_click(field(react, "remoteRepo.select", repo.id.clone().into()))
              .child(
                div()
                  .flex()
                  .items_baseline()
                  .child(repo.display_name.clone())
                  .child(
                    ui::text(div(), TextSize::Xs)
                      .ml_2()
                      .text_color(p.muted_foreground)
                      .child(repo.canonical_remote_path.clone()),
                  ),
              )
          })),
      )
  });
  let path_input = inputs
    .path
    .as_ref()
    .map(|f| input("remote-repo-path", &f.state, p));
  let probe_actions = div()
    .flex()
    .flex_wrap()
    .gap_2()
    .child(
      Button::new("remote-repo-probe", p)
        .size(Size::Sm)
        .disabled(form.busy || path.trim().is_empty())
        .label("Probe")
        .on_click(press(react, "remoteRepo.probe")),
    )
    .when(
      form.probe.as_ref().is_some_and(|probe| probe.is_repo),
      |d| {
        d.child(
          Button::new("remote-repo-open", p)
            .size(Size::Sm)
            .disabled(form.busy)
            .label("Open")
            .on_click(press(react, "remoteRepo.open")),
        )
      },
    );
  let not_repo = form
    .probe
    .as_ref()
    .filter(|probe| !probe.is_repo)
    .map(|probe| {
      let confirm = form.confirm_init;
      div()
        .debug_selector(|| "remote-repo-not-a-repo".into())
        .flex()
        .flex_col()
        .gap_2()
        .rounded(zpx(8.))
        .border_1()
        .border_color(p.border.opacity(0.6))
        .p_3()
        .when(probe.needs_clone, |d| {
          d.child(
            div()
              .flex()
              .flex_col()
              .gap_1()
              .child(caption("Git URL to clone"))
              .children(
                inputs
                  .clone_url
                  .as_ref()
                  .map(|f| input("remote-repo-clone-url", &f.state, p)),
              ),
          )
          .child(
            div().flex().child(
              Button::new("remote-repo-clone", p)
                .size(Size::Sm)
                .disabled(form.busy || clone_url.trim().is_empty())
                .label("Clone repository")
                .on_click(press(react, "remoteRepo.clone")),
            ),
          )
        })
        .child(
          ui::text(div(), TextSize::Sm)
            .id("remote-repo-confirm-init")
            .debug_selector(|| "remote-repo-confirm-init".into())
            .flex()
            .items_center()
            .gap_2()
            .cursor_pointer()
            .on_click(field(react, "remoteRepo.confirmInit", (!confirm).into()))
            .child(super::workspace_modal::checkbox(
              "remote-repo-confirm-init-box",
              if confirm {
                super::workspace_modal::Check::On
              } else {
                super::workspace_modal::Check::Off
              },
              false,
              p,
            ))
            .child("Initialize an empty repository at this path"),
        )
        .child(
          div().flex().child(
            Button::new("remote-repo-init", p)
              .size(Size::Sm)
              .disabled(form.busy || !confirm)
              .label("Initialize repository")
              .on_click(press(react, "remoteRepo.init")),
          ),
        )
    });
  let error = form.error.clone().map(|error| {
    ui::text(div(), TextSize::Sm)
      .debug_selector(|| "remote-repo-error".into())
      .text_color(tw::color(if dark { tw::RED_400 } else { tw::RED_600 }))
      .child(error)
  });
  div()
    .debug_selector(|| "remote-repository-selector".into())
    .flex()
    .flex_col()
    .gap_3()
    .children(saved)
    .child(
      div()
        .flex()
        .flex_col()
        .gap_1()
        .max_w(zpx(448.))
        .child(caption("Remote repository path"))
        .children(path_input),
    )
    .child(probe_actions)
    .children(not_repo)
    .children(error)
}

/// The form: the cut-off notice, or the selector.
pub fn render(
  form: &Form,
  inputs: &Inputs,
  react: &ActionSink,
  dark: bool,
  p: &Palette,
  cx: &App,
) -> Div {
  let cutoff = form.cutoff.as_ref().map(|reason| {
    let red: gpui::Hsla = tw::color(tw::RED_500);
    ui::text(div(), TextSize::Sm)
      .debug_selector(|| "remote-repo-cutoff".into())
      .rounded(zpx(8.))
      .border_1()
      .border_color(red.opacity(0.4))
      .bg(red.opacity(0.1))
      .px_3()
      .py_2()
      .child(
        div()
          .font_weight(FontWeight::MEDIUM)
          .text_color(tw::color(if dark { tw::RED_300 } else { tw::RED_700 }))
          .child(cutoff_title(reason)),
      )
      .child(div().mt_1().text_color(p.muted_foreground).child(
        "This instance is cut off from further commands until you reauthenticate and obtain a new certificate.",
      ))
      .child(
        div().flex().mt_2().child(
          Button::new("remote-repo-reauthenticate", p)
            .size(Size::Sm)
            .label("Reauthenticate")
            .on_click(press(react, "remoteRepo.reauthenticate")),
        ),
      )
  });
  let show_selector = form.selector && form.cutoff.is_none();
  div()
    .debug_selector(|| "remote-repo-form".into())
    .flex()
    .flex_col()
    .gap_3()
    .px_6()
    .pb_6()
    .children(cutoff)
    .when(show_selector, |d| {
      d.child(selector(form, inputs, react, dark, p, cx))
    })
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn names_the_cutoff_like_react() {
    assert_eq!(
      cutoff_title("session_ended"),
      "Access blocked (session ended)"
    );
    assert_eq!(cutoff_title("key_revoked"), "Access blocked (key revoked)");
  }
}
