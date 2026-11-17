//! A path chosen with the system's file picker, never typed.

use std::{path::PathBuf, rc::Rc};

use gpui::{div, prelude::*, App, ExternalPaths, PathPromptOptions, SharedString, Window};

use crate::{text, theme, theme::Palette, zpx, Button, Size, TextSize, Variant};

type OnPick = Rc<dyn Fn(PathBuf, &mut Window, &mut App)>;

/// The chosen path, read-only in mono, and "Browse…", which opens the
/// native picker. Dropping a file or folder from the OS on it picks that.
#[derive(IntoElement)]
pub struct PathField {
  id: SharedString,
  palette: Palette,
  value: Option<SharedString>,
  placeholder: SharedString,
  directories: bool,
  disabled: bool,
  on_pick: Option<OnPick>,
  browse_label: SharedString,
  on_browse: Option<crate::touch::Action>,
}

impl PathField {
  pub fn new(id: impl Into<SharedString>, palette: &Palette) -> Self {
    Self {
      id: id.into(),
      palette: *palette,
      value: None,
      placeholder: "No folder chosen".into(),
      directories: true,
      disabled: false,
      on_pick: None,
      browse_label: "Browse…".into(),
      on_browse: None,
    }
  }

  pub fn value(mut self, value: Option<impl Into<SharedString>>) -> Self {
    self.value = value.map(Into::into);
    self
  }

  pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
    self.placeholder = placeholder.into();
    self
  }

  /// Picks a file instead of a folder.
  pub fn files(mut self) -> Self {
    self.directories = false;
    if self.placeholder.as_ref() == "No folder chosen" {
      self.placeholder = "No file chosen".into();
    }
    self
  }

  pub fn disabled(mut self, disabled: bool) -> Self {
    self.disabled = disabled;
    self
  }

  pub fn on_pick(mut self, f: impl Fn(PathBuf, &mut Window, &mut App) + 'static) -> Self {
    self.on_pick = Some(Rc::new(f));
    self
  }

  /// Browses somewhere the system picker can't see, such as a remote
  /// host's disk on a phone: the button runs `f`, which calls back with
  /// the path, instead of opening the picker.
  pub fn on_browse(
    mut self,
    label: impl Into<SharedString>,
    f: impl Fn(&mut Window, &mut App) + 'static,
  ) -> Self {
    self.browse_label = label.into();
    self.on_browse = Some(Rc::new(f));
    self
  }

  fn options(&self) -> PathPromptOptions {
    PathPromptOptions {
      files: !self.directories,
      directories: self.directories,
      multiple: false,
      prompt: None,
    }
  }
}

/// The dropped path it takes: the first, if it is the right kind.
fn dropped(paths: &[PathBuf], directories: bool) -> Option<PathBuf> {
  paths.first().filter(|p| p.is_dir() == directories).cloned()
}

impl RenderOnce for PathField {
  fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
    let p = self.palette;
    let density = theme::Density::current(cx);
    let options = self.options();
    let directories = self.directories;
    let on_pick = self.on_pick.filter(|_| !self.disabled);
    let shown = self.value.is_some();
    let selector = format!("{}-value", self.id);
    let enabled = !self.disabled && (on_pick.is_some() || self.on_browse.is_some());
    // A custom browse (a remote host's folders) or the system picker.
    let click: Option<crate::touch::Action> = if !enabled {
      None
    } else if let Some(f) = self.on_browse.clone() {
      Some(f)
    } else {
      on_pick.clone().map(|f| -> crate::touch::Action {
        std::rc::Rc::new(move |window: &mut Window, cx: &mut App| {
          let rx = cx.prompt_for_paths(options.clone());
          let f = f.clone();
          window
            .spawn(cx, async move |cx| {
              if let Ok(Ok(Some(paths))) = rx.await {
                if let Some(path) = paths.into_iter().next() {
                  cx.update(|window, cx| f(path, window, cx)).ok();
                }
              }
            })
            .detach();
        })
      })
    };
    let value = div()
      .debug_selector(move || selector)
      .flex_1()
      .min_w_0()
      .h_full()
      .px_2()
      .flex()
      .items_center()
      .bg(p.muted.opacity(0.5))
      .font_family(theme::MONO_FONT)
      .map(|el| text(el, TextSize::Code))
      .text_color(if shown {
        p.foreground
      } else {
        p.muted_foreground
      })
      .overflow_hidden()
      .whitespace_nowrap()
      .text_ellipsis()
      .child(self.value.unwrap_or(self.placeholder));
    let browse_id = format!("{}-browse", self.id);
    let body = match density {
      // On a phone the action is an icon joined to the field.
      theme::Density::Touch => crate::InputGroup::new(self.id.clone(), &p)
        .control(value)
        .append(
          crate::IconButton::new(
            browse_id,
            theme::ICON_FOLDER_OPEN,
            self.browse_label.clone(),
            &p,
          )
          .disabled(!enabled)
          .when_some(click, |b, f| b.on_click(move |_, window, cx| f(window, cx))),
        )
        .into_any_element(),
      theme::Density::Desktop => div()
        .flex()
        .gap(zpx(theme::space::SM))
        .child(
          value
            .h(zpx(density.control_height()))
            .rounded(zpx(theme::RADIUS_MD))
            .border_1()
            .border_color(p.input),
        )
        .child(
          Button::new(browse_id, &p)
            .variant(Variant::Outline)
            .size(Size::Md)
            .label(self.browse_label.clone())
            .disabled(!enabled)
            .when_some(click, |b, f| b.on_click(move |_, window, cx| f(window, cx))),
        )
        .into_any_element(),
    };
    div()
      .id(SharedString::from(format!("{}-drop", self.id)))
      .min_w_0()
      .when_some(on_pick, |el, f| {
        el.on_drop(move |paths: &ExternalPaths, window, cx| {
          if let Some(path) = dropped(paths.paths(), directories) {
            f(path, window, cx)
          }
        })
      })
      .child(body)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  struct Host;

  impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
      let p = theme::palette(false);
      div().w(gpui::px(300.)).child(
        PathField::new("clone", &p)
          .value(Some("~/code"))
          .on_pick(|_, _, _| {}),
      )
    }
  }

  fn addon(cx: &mut gpui::TestAppContext, density: theme::Density) -> bool {
    cx.update(|cx| {
      theme::init(cx);
      theme::set_density(density, cx);
    });
    let window = cx.add_window(|_, _| Host);
    let mut cx = gpui::VisualTestContext::from_window(*window, cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.debug_bounds("clone-addon-0").is_some()
  }

  #[gpui::test]
  fn on_touch_browse_is_an_icon_button_joined_to_the_field(cx: &mut gpui::TestAppContext) {
    assert!(addon(cx, theme::Density::Touch));
  }

  #[gpui::test]
  fn on_desktop_browse_is_a_labelled_button_beside_it(cx: &mut gpui::TestAppContext) {
    assert!(!addon(cx, theme::Density::Desktop));
  }

  #[test]
  fn a_custom_browse_replaces_the_system_picker() {
    let p = theme::palette(false);
    let f = PathField::new("f", &p);
    assert!(f.on_browse.is_none());
    assert_eq!(f.browse_label.as_ref(), "Browse…");
    let f = f.on_browse("Browse host…", |_, _| {});
    assert!(f.on_browse.is_some());
    assert_eq!(f.browse_label.as_ref(), "Browse host…");
  }

  #[test]
  fn it_picks_a_folder_unless_told_files() {
    let p = theme::palette(false);
    let o = PathField::new("f", &p).options();
    assert!(o.directories && !o.files && !o.multiple);
    let f = PathField::new("f", &p).files();
    assert!(f.options().files && !f.options().directories);
    assert_eq!(f.placeholder.as_ref(), "No file chosen");
  }

  #[test]
  fn a_drop_takes_the_first_path_of_the_right_kind() {
    let dir = std::env::temp_dir();
    let file = dir.join("treq-ui-path-field-test");
    std::fs::write(&file, b"").unwrap();
    assert_eq!(
      dropped(&[dir.clone(), file.clone()], true),
      Some(dir.clone())
    );
    assert_eq!(dropped(std::slice::from_ref(&file), true), None);
    assert_eq!(
      dropped(std::slice::from_ref(&file), false),
      Some(file.clone())
    );
    assert_eq!(dropped(&[], true), None);
    std::fs::remove_file(file).ok();
  }
}
