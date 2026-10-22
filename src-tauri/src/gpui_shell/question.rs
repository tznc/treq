//! Yes/No and OK/Cancel questions in a native dialog, owned by the window
//! that asks so it stays above that window. A window asks one question at a
//! time: asking again while one is open answers no, so a second click on
//! Delete can't stack another dialog.

use std::{
  collections::HashSet,
  sync::{Mutex, OnceLock},
};

use super::native::Backend;
use crate::lock_ext::LockExt;

/// The buttons a question offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Buttons {
  YesNo,
  OkCancel,
}

/// The window labels with a question open.
fn open() -> &'static Mutex<HashSet<String>> {
  static OPEN: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
  OPEN.get_or_init(Mutex::default)
}

/// Window `label`'s open question; dropping it lets the window ask again.
#[derive(Debug)]
struct Asking(String);

impl Asking {
  /// `None` while the window already has a question open.
  fn start(label: &str) -> Option<Self> {
    open()
      .lock_or_recover()
      .insert(label.to_string())
      .then(|| Self(label.to_string()))
  }
}

impl Drop for Asking {
  fn drop(&mut self) {
    open().lock_or_recover().remove(&self.0);
  }
}

impl Backend {
  /// Asks `title` with `message` over this window and waits for the answer,
  /// so call it off the main thread. Answers no if the window already has a
  /// question open.
  pub fn ask_native(&self, title: &str, message: &str, buttons: Buttons) -> bool {
    let Some(_asking) = Asking::start(self.label()) else {
      return false;
    };
    let (tx, rx) = std::sync::mpsc::channel();
    let app = self.app().clone();
    let label = self.label().to_string();
    let (title, message) = (title.to_string(), message.to_string());
    // The dialog opens from the main thread; this waits off it.
    let _ = self.app().run_on_main_thread(move || {
      let parent = tauri::Manager::get_window(&app, &label);
      show(
        &app,
        parent.as_ref(),
        &title,
        &message,
        buttons,
        move |yes| {
          let _ = tx.send(yes);
        },
      );
    });
    rx.recv().unwrap_or(false)
  }
}

/// rfd's GTK dialogs ignore their parent, so on Linux GTK shows the dialog
/// itself, modal and transient for the window.
#[cfg(target_os = "linux")]
fn show(
  app: &tauri::AppHandle,
  parent: Option<&tauri::Window>,
  title: &str,
  message: &str,
  buttons: Buttons,
  answer: impl Fn(bool) + Send + 'static,
) {
  use gtk::prelude::*;
  let Some(parent) = parent.and_then(|window| window.gtk_window().ok()) else {
    return show_unowned(app, None, title, message, buttons, answer);
  };
  let dialog = gtk::MessageDialog::new(
    Some(&parent),
    gtk::DialogFlags::MODAL | gtk::DialogFlags::DESTROY_WITH_PARENT,
    gtk::MessageType::Warning,
    match buttons {
      Buttons::YesNo => gtk::ButtonsType::YesNo,
      Buttons::OkCancel => gtk::ButtonsType::OkCancel,
    },
    title,
  );
  dialog.set_title(title);
  dialog.set_secondary_text(Some(message));
  dialog.connect_response(move |dialog, response| {
    answer(matches!(
      response,
      gtk::ResponseType::Yes | gtk::ResponseType::Ok
    ));
    // SAFETY: nothing else holds the dialog once it has answered.
    unsafe { dialog.destroy() };
  });
  dialog.show_all();
}

#[cfg(not(target_os = "linux"))]
fn show(
  app: &tauri::AppHandle,
  parent: Option<&tauri::Window>,
  title: &str,
  message: &str,
  buttons: Buttons,
  answer: impl Fn(bool) + Send + 'static,
) {
  show_unowned(app, parent, title, message, buttons, answer);
}

/// The dialog plugin's dialog, owned by `parent` where the platform allows.
fn show_unowned(
  app: &tauri::AppHandle,
  parent: Option<&tauri::Window>,
  title: &str,
  message: &str,
  buttons: Buttons,
  answer: impl Fn(bool) + Send + 'static,
) {
  use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
  let dialog = app
    .dialog()
    .message(message)
    .title(title)
    .kind(MessageDialogKind::Warning)
    .buttons(match buttons {
      Buttons::YesNo => MessageDialogButtons::YesNo,
      Buttons::OkCancel => MessageDialogButtons::OkCancel,
    });
  match parent {
    Some(parent) => dialog.parent(parent),
    None => dialog,
  }
  .show(answer);
}

#[cfg(test)]
mod tests {
  use super::Asking;

  #[test]
  fn a_window_asks_one_question_at_a_time() {
    let first = Asking::start("question-test-a").expect("first question");
    assert!(Asking::start("question-test-a").is_none());
    let other = Asking::start("question-test-b");
    assert!(other.is_some(), "another window asks on its own");
    drop(first);
    assert!(Asking::start("question-test-a").is_some());
  }
}
