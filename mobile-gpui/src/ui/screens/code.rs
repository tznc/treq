//! The workspace's files, for its Code tab: one folder at a time, or the
//! files whose path matches a search. Comments left in the file viewer
//! batch into the tab's review, sent from the bar at its bottom.

use std::time::Duration;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
  component::{
    input::{Input, InputEvent, InputState},
    v_flex, ActiveTheme, IconName, Sizable,
  },
  *,
};
use treq_lib::core::remote::{
  RemoteFileEntry, RemoteFileSearchResult, ReviewScope, TreqCommandRequest,
};

use super::{file, terminal::Sessions, workspace::Target};
use crate::{
  errors, remote,
  ui::{
    load,
    review::{self, Handler, Review},
    widgets, Load,
  },
};

/// How long typing pauses before a search goes to the host.
const SEARCH_DELAY: Duration = Duration::from_millis(250);

pub struct CodeView {
  target: Target,
  sessions: Entity<Sessions>,
  /// The review written in the file viewer, kept apart from Changes'.
  review: Entity<Review>,
  /// Switches the workspace to its Changes tab.
  show_changes: Handler,
  /// The folder shown, relative to the workspace; empty at its root.
  dir: String,
  listing: Load<Vec<RemoteFileEntry>>,
  query: Entity<InputState>,
  /// Matches for the search; `Idle` while the search is empty.
  matches: Load<Vec<String>>,
  /// Bumped per keystroke, so only the last pause searches.
  search_seq: usize,
  scroll: ScrollHandle,
}

fn list_request(target: &Target, dir: &str) -> TreqCommandRequest {
  TreqCommandRequest::ListFiles {
    repo: target.repo.clone(),
    workspace: Some(target.id.clone()),
    path: dir.to_string(),
  }
}

/// "src/ui/mod.rs" → ("mod.rs", "src/ui").
pub fn split_path(path: &str) -> (&str, &str) {
  match path.rsplit_once('/') {
    Some((dir, name)) => (name, dir),
    None => (path, ""),
  }
}

/// The folder above `dir`; the root's is the root.
fn parent_dir(dir: &str) -> String {
  split_path(dir).1.to_string()
}

/// A row's second line: its status, then its comments in the review.
fn row_detail(status: Option<&str>, comments: usize) -> Option<SharedString> {
  let comments = (comments > 0).then(|| match comments {
    1 => "1 comment".to_string(),
    n => format!("{n} comments"),
  });
  match (status, comments) {
    (Some(s), Some(c)) => Some(format!("{s} · {c}").into()),
    (Some(s), None) => Some(s.into()),
    (None, Some(c)) => Some(c.into()),
    (None, None) => None,
  }
}

/// What a file's status says on its row.
pub fn status_label(status: &str) -> Option<&'static str> {
  match status {
    "conflict" => Some("Conflicted"),
    "workingCopy" => Some("Uncommitted changes"),
    "committed" => Some("Changed on this branch"),
    _ => None,
  }
}

impl CodeView {
  pub fn new(
    target: Target,
    sessions: Entity<Sessions>,
    show_changes: Handler,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Self {
    let query = cx.new(|cx| InputState::new(window, cx).placeholder("Search files"));
    cx.subscribe(&query, |this, _, event: &InputEvent, cx| {
      if let InputEvent::Change = event {
        this.search_changed(cx);
      }
    })
    .detach();
    let review = cx.new(|cx| Review::new(target.clone(), ReviewScope::Files, cx));
    cx.observe(&review, |_, _, cx| cx.notify()).detach();
    let listing = remote::last(&list_request(&target, "")).map_or(Load::Idle, Load::Ready);
    let mut this = Self {
      target,
      sessions,
      review,
      show_changes,
      dir: String::new(),
      listing,
      query,
      matches: Load::Idle,
      search_seq: 0,
      scroll: ScrollHandle::new(),
    };
    this.fetch(true, cx);
    this
  }

  pub fn reload(&mut self, cx: &mut Context<Self>) {
    self.fetch(false, cx);
    if !self.query_text(cx).is_empty() {
      self.search(cx);
    }
  }

  /// `join` as in [`remote::read_cached`]. A folder seen before shows at
  /// once and is read again.
  fn fetch(&mut self, join: bool, cx: &mut Context<Self>) {
    let request = list_request(&self.target, &self.dir);
    self.listing = remote::last(&request).map_or(Load::Loading, Load::Ready);
    let dir = self.dir.clone();
    load::run(
      cx,
      remote::read_cached::<Vec<RemoteFileEntry>>(request, join),
      move |this, result, _| {
        // A reply for a folder already left is dropped.
        if this.dir == dir {
          this.listing = Load::from_result(result);
        }
      },
    );
  }

  fn open_dir(&mut self, dir: String, cx: &mut Context<Self>) {
    self.dir = dir;
    self.scroll.set_offset(point(px(0.), px(0.)));
    self.fetch(true, cx);
    cx.notify();
  }

  fn open_file(&self, path: String, status: Option<String>, cx: &mut Context<Self>) {
    file::open(
      file::Opened {
        target: self.target.clone(),
        path,
        status,
        sessions: self.sessions.clone(),
        review: self.review.clone(),
        show_changes: self.show_changes.clone(),
      },
      cx,
    );
  }

  fn query_text(&self, cx: &App) -> String {
    self.query.read(cx).value().trim().to_string()
  }

  fn search_changed(&mut self, cx: &mut Context<Self>) {
    self.search_seq += 1;
    if self.query_text(cx).is_empty() {
      self.matches = Load::Idle;
      cx.notify();
      return;
    }
    let seq = self.search_seq;
    cx.spawn(async move |this, cx| {
      cx.background_executor().timer(SEARCH_DELAY).await;
      this
        .update(cx, |this, cx| {
          if this.search_seq == seq {
            this.search(cx);
          }
        })
        .ok();
    })
    .detach();
  }

  fn search(&mut self, cx: &mut Context<Self>) {
    let query = self.query_text(cx);
    if self.matches.ready().is_none() {
      self.matches = Load::Loading;
    }
    let seq = self.search_seq;
    let request = TreqCommandRequest::SearchFiles {
      repo: self.target.repo.clone(),
      workspace: Some(self.target.id.clone()),
      query,
      limit: Some(50),
    };
    load::run(
      cx,
      remote::read::<Vec<RemoteFileSearchResult>>(request),
      move |this, result, _| {
        if this.search_seq == seq {
          this.matches = Load::from_result(
            result.map(|found| found.into_iter().map(|f| f.relative_path).collect()),
          );
        }
      },
    );
    cx.notify();
  }

  fn status_tone(status: Option<&str>, cx: &App) -> Hsla {
    let theme = cx.theme();
    match status {
      Some("conflict") => theme.danger,
      Some("workingCopy") => theme.warning,
      Some("committed") => theme.info,
      _ => theme.muted_foreground,
    }
  }

  fn folder(&self, cx: &mut Context<Self>) -> AnyElement {
    let entries = match &self.listing {
      Load::Idle | Load::Loading => {
        return widgets::card(cx)
          .child(widgets::centered_message("Loading files…", cx))
          .into_any_element()
      }
      Load::Failed(error) => {
        return widgets::card(cx)
          .p_4()
          .child(widgets::error_text(errors::describe(error).message, cx))
          .into_any_element()
      }
      Load::Ready(entries) => entries,
    };
    let up = (!self.dir.is_empty()).then(|| {
      let parent = parent_dir(&self.dir);
      let leading = div()
        .text_color(cx.theme().muted_foreground)
        .child(IconName::ArrowUp)
        .into_any_element();
      widgets::row_full("files-up", Some(leading), "Up one folder", None, None, cx)
        .on_click(cx.listener(move |this, _, _, cx| this.open_dir(parent.clone(), cx)))
    });
    let rows = entries.iter().enumerate().map(|(i, entry)| {
      let status = entry.status.clone();
      let leading = div()
        .text_color(Self::status_tone(status.as_deref(), cx))
        .child(if entry.is_directory {
          IconName::Folder
        } else {
          IconName::File
        })
        .into_any_element();
      let comments = if entry.is_directory {
        0
      } else {
        self.review.read(cx).count_for(&entry.path)
      };
      let detail = row_detail(status.as_deref().and_then(status_label), comments);
      let path = entry.path.clone();
      let is_directory = entry.is_directory;
      widgets::row_full(
        ("entry", i),
        Some(leading),
        entry.name.clone(),
        detail,
        None,
        cx,
      )
      .on_click(cx.listener(move |this, _, _, cx| {
        if is_directory {
          this.open_dir(path.clone(), cx)
        } else {
          this.open_file(path.clone(), status.clone(), cx)
        }
      }))
    });
    let empty = entries.is_empty();
    widgets::card(cx)
      .children(up)
      .children(rows)
      .when(empty, |card| {
        card.child(widgets::centered_message("This folder is empty.", cx))
      })
      .into_any_element()
  }

  fn matches(&self, cx: &mut Context<Self>) -> AnyElement {
    match &self.matches {
      Load::Idle | Load::Loading => widgets::card(cx)
        .child(widgets::centered_message("Searching…", cx))
        .into_any_element(),
      Load::Failed(error) => widgets::card(cx)
        .p_4()
        .child(widgets::error_text(errors::describe(error).message, cx))
        .into_any_element(),
      Load::Ready(paths) if paths.is_empty() => widgets::card(cx)
        .child(widgets::centered_message("No files match.", cx))
        .into_any_element(),
      Load::Ready(paths) => widgets::card(cx)
        .children(paths.iter().enumerate().map(|(i, path)| {
          let (name, dir) = split_path(path);
          let leading = div()
            .text_color(cx.theme().muted_foreground)
            .child(IconName::File)
            .into_any_element();
          let path = path.clone();
          widgets::row_full(
            ("match", i),
            Some(leading),
            name.to_string(),
            (!dir.is_empty()).then(|| SharedString::from(dir.to_string())),
            None,
            cx,
          )
          .on_click(cx.listener(move |this, _, _, cx| this.open_file(path.clone(), None, cx)))
        }))
        .into_any_element(),
    }
  }
}

impl Render for CodeView {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let searching = !self.query_text(cx).is_empty();
    let location = if self.dir.is_empty() {
      super::workspaces::repo_name(&self.target.repo).to_string()
    } else {
      format!(
        "{} / {}",
        super::workspaces::repo_name(&self.target.repo),
        self.dir.replace('/', " / ")
      )
    };
    let bar = review::bar(&self.review, &self.sessions, cx);
    v_flex()
      .size_full()
      .child(
        div()
          .id("code")
          .flex_1()
          .min_h_0()
          .overflow_y_scroll()
          .track_scroll(&self.scroll)
          .child(
            widgets::page()
              .gap_3()
              .child(Input::new(&self.query).large().cleanable(true))
              .child(if searching {
                v_flex()
                  .gap_2()
                  .child(widgets::section("Matching files", cx))
                  .child(self.matches(cx))
              } else {
                v_flex()
                  .gap_2()
                  .child(
                    widgets::mono(location, cx)
                      .text_color(cx.theme().muted_foreground)
                      .truncate(),
                  )
                  .child(self.folder(cx))
              }),
          ),
      )
      .children(bar)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[core::prelude::v1::test]
  fn paths_split_into_name_and_folder() {
    assert_eq!(split_path("src/ui/mod.rs"), ("mod.rs", "src/ui"));
    assert_eq!(split_path("README.md"), ("README.md", ""));
    assert_eq!(parent_dir("src/ui"), "src");
    assert_eq!(parent_dir("src"), "");
  }

  #[core::prelude::v1::test]
  fn rows_add_their_comment_count_to_the_status() {
    assert_eq!(row_detail(None, 0), None);
    assert_eq!(row_detail(Some("Conflicted"), 0), Some("Conflicted".into()));
    assert_eq!(row_detail(None, 1), Some("1 comment".into()));
    assert_eq!(
      row_detail(Some("Uncommitted changes"), 3),
      Some("Uncommitted changes · 3 comments".into())
    );
  }
}
