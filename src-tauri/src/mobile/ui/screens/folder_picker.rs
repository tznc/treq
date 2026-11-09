//! Choosing a folder on the connected host: the phone's translation of
//! desktop's `PathField`, which opens the system picker. A phone can't see
//! the host's disk, so this lists its folders over the connection.

use std::rc::Rc;

use gpui_kit::{
  component::{
    button::{Button, ButtonVariants},
    v_flex, Sizable,
  },
  div, App, AppContext, Context, IntoElement, ParentElement, Render, Styled, Window,
};

use crate::core::remote::{DirectoryEntry, DirectoryListing, TreqCommandRequest};
use crate::mobile::{
  errors, remote,
  ui::{load, sheet, widgets, widgets::TouchButton as _, Load},
};

type OnPick = Rc<dyn Fn(String, &mut Window, &mut App)>;

/// Opens the picker at `start` (`~` when empty); `on_pick` gets the folder.
pub fn open(
  start: String,
  on_pick: impl Fn(String, &mut Window, &mut App) + 'static,
  cx: &mut App,
) {
  let start = if start.trim().is_empty() {
    "~".to_string()
  } else {
    start
  };
  let on_pick: OnPick = Rc::new(on_pick);
  sheet::open(Some("Choose a folder".into()), cx, move |_, cx| {
    cx.new(|cx| {
      let mut picker = FolderPicker {
        listing: Load::Idle,
        on_pick,
      };
      picker.go(start, cx);
      picker
    })
  });
}

struct FolderPicker {
  listing: Load<DirectoryListing>,
  on_pick: OnPick,
}

/// The folders to list: hidden ones left out, repositories first.
fn visible(entries: &[DirectoryEntry]) -> Vec<&DirectoryEntry> {
  let mut shown: Vec<_> = entries.iter().filter(|e| !e.hidden).collect();
  shown.sort_by_key(|e| !e.is_repo);
  shown
}

impl FolderPicker {
  fn go(&mut self, path: String, cx: &mut Context<Self>) {
    self.listing = Load::Loading;
    load::run(
      cx,
      remote::read::<DirectoryListing>(TreqCommandRequest::ListDirectories { path }),
      |this, result, cx| {
        this.listing = Load::from_result(result);
        cx.notify();
      },
    );
    cx.notify();
  }
}

impl Render for FolderPicker {
  fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let listing = match &self.listing {
      Load::Idle | Load::Loading => {
        return div()
          .child(widgets::skeleton("folders", 5, cx))
          .into_any_element()
      }
      Load::Failed(error) => {
        return div()
          .child(
            widgets::empty("folders-failed", errors::describe(error).message, cx)
              .error(true)
              .icon(treq_ui::theme::ICON_CIRCLE_ALERT),
          )
          .into_any_element()
      }
      Load::Ready(listing) => listing.clone(),
    };
    let up = listing.parent.clone().map(|parent| {
      widgets::row("folder-up", "..", Some(parent.clone().into()), cx)
        .on_click(cx.listener(move |this, _, _, cx| this.go(parent.clone(), cx)))
    });
    let rows = visible(&listing.entries)
      .into_iter()
      .enumerate()
      .map(|(i, entry)| {
        let path = entry.path.clone();
        let detail = entry.is_repo.then(|| "Repository".into());
        widgets::row(format!("folder-{i}"), entry.name.clone(), detail, cx)
          .on_click(cx.listener(move |this, _, _, cx| this.go(path.clone(), cx)))
      })
      .collect::<Vec<_>>();
    let on_pick = self.on_pick.clone();
    let chosen = listing.path.clone();
    v_flex()
      .child(widgets::mono(listing.path, cx).px_4().pb_2())
      .children(up)
      .children(rows)
      .child(
        div().px_4().pt_3().child(
          Button::new("folder-choose")
            .primary()
            .large()
            .w_full()
            .label("Choose this folder")
            .on_click(move |_, window, cx| {
              sheet::close(cx);
              on_pick(chosen.clone(), window, cx);
            })
            .touch(),
        ),
      )
      .into_any_element()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn entry(name: &str, hidden: bool, is_repo: bool) -> DirectoryEntry {
    DirectoryEntry {
      name: name.into(),
      path: format!("/home/ty/{name}"),
      hidden,
      is_repo,
    }
  }

  #[test]
  fn hidden_folders_are_left_out_and_repositories_come_first() {
    let entries = [
      entry("notes", false, false),
      entry(".cache", true, false),
      entry("treq", false, true),
    ];
    let names: Vec<_> = visible(&entries).iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["treq", "notes"]);
  }
}
