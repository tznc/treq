//! Dashboard's explicit-endpoint repository form, under its machine's group
//! on the Overview screen while an SSH endpoint is connected but no
//! repository is open: the cut-off notice with Reauthenticate, or
//! a directory browser that starts at the host's home, with clone and init
//! for a folder that holds no repository. The group lists the endpoint's
//! saved repositories.
//!
//! The window keeps the form's state and runs probe, open, clone and init: GPUI
//! draws what the route says and sends what is typed and pressed
//! (`remoteRepo.*`).

use crate::gpui_shell::ui::radius;
use crate::gpui_shell::ui::zpx;
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::input::{InputEvent, InputState};
use serde::Deserialize;
use tauri_plugin_gpui::gpui::{
  self, div, prelude::*, App, ClickEvent, Context, Div, Entity, FontWeight, Subscription, Window,
};

use super::{ActionSink, ScreenAction};
use crate::gpui_shell::{
  theme::{self, Palette},
  ui::{self, Button, Size, TextSize, Tone},
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

/// A directory on the host (`DirectoryEntry`).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Folder {
  pub name: String,
  pub path: String,
  pub hidden: bool,
  pub is_repo: bool,
}

/// What the host listed at the browsed path (`DirectoryListing`).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Listing {
  pub path: String,
  pub parent: Option<String>,
  pub is_repo: bool,
  pub entries: Vec<Folder>,
}

impl Listing {
  /// The folders to show: hidden ones only when asked.
  pub fn shown(&self, show_hidden: bool) -> impl Iterator<Item = &Folder> {
    self
      .entries
      .iter()
      .filter(move |f| show_hidden || !f.hidden)
  }
}

/// `/home/dev/src` as its breadcrumbs: `/`, `home`, `dev`, `src`, each with
/// the path it opens.
pub fn crumbs(path: &str) -> Vec<(String, String)> {
  let mut out = vec![("/".to_string(), "/".to_string())];
  let mut at = String::new();
  for part in path.split('/').filter(|p| !p.is_empty()) {
    at = format!("{at}/{part}");
    out.push((part.to_string(), at.clone()));
  }
  out
}

/// The form's state, as Dashboard keeps it.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Form {
  /// The connected endpoint the form opens repositories on.
  pub endpoint_id: String,
  /// The endpoint's cutoff reason (`session_ended`, ...), if cut off.
  pub cutoff: Option<String>,
  /// The selector shows: not cut off, and no repository connected yet.
  pub selector: bool,
  pub saved: Vec<SavedRepo>,
  pub selected_id: Option<String>,
  pub path: String,
  pub listing: Option<Listing>,
  pub show_hidden: bool,
  pub browsing: bool,
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
    .rounded(radius::MD)
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

/// The directory browser: home and the path's breadcrumbs, Show hidden,
/// and the folders in it; a folder opens on click.
fn browser(form: &Form, react: &ActionSink, p: &Palette) -> Div {
  let crumbs = crumbs(&form.path);
  let last = crumbs.len().saturating_sub(1);
  let crumb_row = div()
    .flex()
    .flex_wrap()
    .items_center()
    .gap_1()
    .min_w_0()
    .flex_1()
    .child(
      ui::IconButton::new("remote-browse-home", theme::ICON_HOUSE, "Home", p)
        .size(ui::IconButtonSize::Small)
        .on_click(field(react, "remoteRepo.browse", "~".into())),
    )
    .children(crumbs.into_iter().enumerate().map(|(i, (name, path))| {
      let current = i == last;
      div()
        .flex()
        .items_center()
        .gap_1()
        .when(i > 0, |d| {
          d.child(ui::icon(theme::ICON_CHEVRON_RIGHT, 12., p.muted_foreground))
        })
        .child(
          ui::text(div(), TextSize::Sm)
            .id(("remote-browse-crumb", i))
            .debug_selector(move || format!("remote-browse-crumb-{i}"))
            .role(gpui::Role::Button)
            .aria_label(format!("Go to {path}"))
            .px_1()
            .rounded(radius::SM)
            .font_family(theme::MONO_FONT)
            .when(current, |d| d.font_weight(FontWeight::MEDIUM))
            .when(!current, |d| {
              d.cursor_pointer()
                .text_color(p.muted_foreground)
                .hover(move |s| s.bg(p.hover()))
                .on_click(field(react, "remoteRepo.browse", path.clone().into()))
            })
            .child(name)
            .test_support(),
        )
    }));
  let show_hidden = form.show_hidden;
  let hidden_toggle = ui::text(div(), TextSize::Sm)
    .id("remote-browse-hidden")
    .debug_selector(|| "remote-browse-hidden".into())
    .role(gpui::Role::CheckBox)
    .aria_label("Show hidden folders")
    .flex()
    .flex_none()
    .items_center()
    .gap_2()
    .cursor_pointer()
    .on_click(field(react, "remoteRepo.showHidden", (!show_hidden).into()))
    .child(ui::Checkbox::new("remote-browse-hidden-box", p).checked(show_hidden))
    .child("Show hidden")
    .test_support();
  let folders: Vec<_> = form
    .listing
    .iter()
    .flat_map(|listing| listing.shown(show_hidden))
    .map(|folder| {
      ui::Row::new(format!("remote-browse-folder-{}", folder.name), p)
        .icon(if folder.is_repo {
          theme::ICON_FOLDER_GIT
        } else {
          theme::ICON_FOLDER
        })
        .title(folder.name.clone())
        .when(folder.is_repo, |row| {
          row.trailing(ui::Badge::new("Repository", p))
        })
        .on_click(field(
          react,
          "remoteRepo.browse",
          folder.path.clone().into(),
        ))
    })
    .collect();
  // Before the first listing: loading, or nothing when it failed (the
  // error shows under the browser).
  let empty = match (&form.listing, form.browsing) {
    _ if !folders.is_empty() => None,
    (None, true) => Some(
      div()
        .p_2()
        .child(ui::Loading::new("remote-browse-loading", p).label("Loading folders…"))
        .into_any_element(),
    ),
    (None, false) => None,
    (Some(_), _) => Some(
      ui::text(div(), TextSize::Sm)
        .p_2()
        .text_color(p.muted_foreground)
        .child("No folders here.")
        .into_any_element(),
    ),
  };
  div()
    .debug_selector(|| "remote-browser".into())
    .flex()
    .flex_col()
    .rounded(radius::MD)
    .border_1()
    .border_color(p.border)
    .bg(p.background)
    .child(
      div()
        .flex()
        .items_center()
        .gap_2()
        .px_2()
        .py_1()
        .border_b_1()
        .border_color(p.border.opacity(0.6))
        .child(crumb_row)
        .child(hidden_toggle),
    )
    .child(
      div()
        .id("remote-browse-folders")
        .debug_selector(|| "remote-browse-folders".into())
        .max_h(zpx(260.))
        .overflow_y_scroll()
        .p_1()
        .when(form.browsing, |d| d.opacity(0.6))
        .children(folders)
        .children(empty),
    )
}

fn selector(form: &Form, inputs: &Inputs, react: &ActionSink, p: &Palette, cx: &App) -> Div {
  let here_is_repo = form.listing.as_ref().is_some_and(|l| l.is_repo);
  let listed = form.listing.is_some();
  let clone_url = inputs
    .clone_url
    .as_ref()
    .map(|f| f.value(cx))
    .unwrap_or_default();
  let folder_actions = div()
    .flex()
    .flex_wrap()
    .gap_2()
    .when(listed && here_is_repo, |d| {
      d.child(
        Button::new("remote-repo-open", p)
          .size(Size::Sm)
          .disabled(form.busy || form.browsing)
          .label("Open")
          .on_click(press(react, "remoteRepo.open")),
      )
    })
    .when(listed && !here_is_repo && form.probe.is_none(), |d| {
      d.child(
        Button::new("remote-repo-use-folder", p)
          .size(Size::Sm)
          .disabled(form.busy || form.browsing)
          .label("Use this folder")
          .on_click(press(react, "remoteRepo.useFolder")),
      )
    });
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
        .rounded(radius::MD)
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
            .child(ui::Checkbox::new("remote-repo-confirm-init-box", p).checked(confirm))
            .child("Initialize an empty repository in this folder"),
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
      .text_color(Tone::Danger.text(p))
      .child(error)
  });
  div()
    .debug_selector(|| "remote-repository-selector".into())
    .flex()
    .flex_col()
    .gap_3()
    .child(browser(form, react, p))
    .child(folder_actions)
    .children(not_repo)
    .children(error)
}

/// The form: the cut-off notice, or the selector.
pub fn render(
  form: &Form,
  inputs: &Inputs,
  react: &ActionSink,
  _dark: bool,
  p: &Palette,
  cx: &App,
) -> Div {
  let cutoff = form.cutoff.as_ref().map(|reason| {
    ui::text(div(), TextSize::Sm)
      .debug_selector(|| "remote-repo-cutoff".into())
      .rounded(radius::MD)
      .border_1()
      .border_color(Tone::Danger.border(p))
      .bg(Tone::Danger.tint(p))
      .px_3()
      .py_2()
      .child(
        div()
          .font_weight(FontWeight::MEDIUM)
          .text_color(Tone::Danger.text(p))
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
    .children(cutoff)
    .when(show_selector, |d| {
      d.child(selector(form, inputs, react, p, cx))
    })
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn splits_a_path_into_breadcrumbs() {
    assert_eq!(
      crumbs("/home/dev/src"),
      [
        ("/".to_string(), "/".to_string()),
        ("home".into(), "/home".into()),
        ("dev".into(), "/home/dev".into()),
        ("src".into(), "/home/dev/src".into()),
      ]
    );
    assert_eq!(crumbs("/"), [("/".to_string(), "/".to_string())]);
  }

  #[test]
  fn names_the_cutoff_like_react() {
    assert_eq!(
      cutoff_title("session_ended"),
      "Access blocked (session ended)"
    );
    assert_eq!(cutoff_title("key_revoked"), "Access blocked (key revoked)");
  }
}
