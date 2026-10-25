//! The workspace's commits, for its Commits tab. Tapping one of the
//! workspace's own commits opens the same actions as the desktop commit
//! menu: edit description, undo (the tip only) and delete, which can be
//! undone from the bar that follows.

use std::collections::HashSet;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
  component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::{Textarea, TextareaState},
    tag::Tag,
    v_flex, ActiveTheme, Sizable,
  },
  *,
};
use treq_lib::{
  core::remote::TreqCommandRequest,
  jj::{JjLogCommit, JjLogResult},
};

use super::{mutation_message, workspace::Target};
use crate::{
  errors,
  remote::{self, IdempotencyKeys, Mutation},
  ui::{load, sheet, widgets, Load},
};

/// What can be done to a commit, as in the desktop commit menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
  EditDescription,
  Undo,
  Delete,
}

/// The actions for `log[index]`. Like desktop: only the workspace's own
/// commits have any, only the first of them can be undone, and a commit
/// being removed can't be removed again.
pub fn actions(log: &[JjLogCommit], index: usize, removing: &HashSet<String>) -> Vec<Action> {
  let commit = &log[index];
  if commit.on_target_only {
    return vec![];
  }
  let first = log.iter().position(|c| !c.on_target_only) == Some(index);
  let removed = removing.contains(&commit.commit_id);
  let mut actions = vec![Action::EditDescription];
  if first && !removed {
    actions.push(Action::Undo);
  }
  if !removed {
    actions.push(Action::Delete);
  }
  actions
}

/// The log without the commits being removed, which stay hidden until the
/// next read drops them.
fn visible(log: &[JjLogCommit], removing: &HashSet<String>) -> Vec<JjLogCommit> {
  log
    .iter()
    .filter(|c| !removing.contains(&c.commit_id))
    .cloned()
    .collect()
}

/// The operation id an abandon returned, for its undo. A replayed abandon
/// returns none.
fn operation_of(outcome: &Result<Mutation, String>) -> Option<String> {
  match outcome {
    Ok(Mutation::Applied(value)) => value.as_str().filter(|s| !s.is_empty()).map(String::from),
    _ => None,
  }
}

fn first_line(commit: &JjLogCommit) -> String {
  let line = commit.description.lines().next().unwrap_or("").trim();
  if line.is_empty() {
    "(no description)".into()
  } else {
    line.into()
  }
}

/// The bar under the list after an action: what happened, and for a
/// delete, the operation that undoes it.
#[derive(Clone)]
struct Notice {
  ok: bool,
  text: String,
  undo: Option<(String, String)>,
}

/// Emitted after a commit action that may have changed the host, so the
/// workspace screen reloads its status, changes and this list (desktop's
/// `CommitsScreen::changed`).
pub struct Changed;

pub struct CommitsScreen {
  target: Target,
  log: Load<JjLogResult>,
  scroll: ScrollHandle,
  /// Commits abandoned or undone, until the next read drops them.
  removing: HashSet<String>,
  notice: Option<Notice>,
  keys: IdempotencyKeys,
}

impl CommitsScreen {
  pub fn new(target: Target, cx: &mut Context<Self>) -> Self {
    // Its workspace screen reloads it (on a new epoch too), so it doesn't
    // watch the epoch itself. The log is usually prefetched (see
    // [`prefetch`]), or shown from the last visit.
    let log = remote::last(&request(&target)).map_or(Load::Idle, Load::Ready);
    let mut this = Self {
      target,
      log,
      scroll: ScrollHandle::new(),
      removing: HashSet::new(),
      notice: None,
      keys: IdempotencyKeys::default(),
    };
    this.fetch(true, cx);
    this
  }

  pub fn reload(&mut self, cx: &mut Context<Self>) {
    self.fetch(false, cx);
  }

  /// `join` as in [`remote::read_cached`]. Keeps the commits on screen
  /// while they're read again.
  fn fetch(&mut self, join: bool, cx: &mut Context<Self>) {
    if self.log.ready().is_none() {
      self.log = Load::Loading;
    }
    load::run(
      cx,
      remote::read_cached::<JjLogResult>(request(&self.target), join),
      |this, result, _| {
        if let Ok(log) = &result {
          let ids: HashSet<&str> = log.commits.iter().map(|c| c.commit_id.as_str()).collect();
          this.removing.retain(|id| ids.contains(id.as_str()));
        }
        this.log = Load::from_result(result);
      },
    );
  }

  fn menu(&mut self, commit: JjLogCommit, actions: Vec<Action>, cx: &mut Context<Self>) {
    let this = cx.weak_entity();
    let entries = actions
      .into_iter()
      .map(|action| {
        let (this, commit) = (this.clone(), commit.clone());
        let run = move |window: &mut Window, cx: &mut App| {
          let commit = commit.clone();
          this
            .update(cx, |screen, cx| match action {
              Action::EditDescription => screen.edit_description(commit, window, cx),
              Action::Undo => screen.confirm_undo(commit, cx),
              Action::Delete => screen.confirm_delete(commit, cx),
            })
            .ok();
        };
        match action {
          Action::EditDescription => {
            sheet::MenuEntry::new("commit-describe", "Edit description", run)
          }
          Action::Undo => sheet::MenuEntry::new("commit-undo", "Undo commit", run),
          Action::Delete => sheet::MenuEntry::new("commit-delete", "Delete commit", run).danger(),
        }
      })
      .collect();
    sheet::menu(
      format!("{} · {}", commit.short_id, first_line(&commit)),
      entries,
      cx,
    );
  }

  fn edit_description(&mut self, commit: JjLogCommit, _: &mut Window, cx: &mut Context<Self>) {
    let screen = cx.weak_entity();
    sheet::open(Some("Edit description".into()), cx, move |window, cx| {
      cx.new(|cx| DescribeSheet::new(commit, screen, window, cx))
    });
  }

  fn save_description(
    &mut self,
    commit: &JjLogCommit,
    description: String,
    cx: &mut Context<Self>,
  ) {
    if description == commit.description {
      return;
    }
    // The row shows the new description at once; a failure puts the old
    // one back.
    let before = self.set_description(&commit.change_id, &description);
    let request = TreqCommandRequest::DescribeCommit {
      repo: self.target.repo.clone(),
      workspace: self.target.id.clone(),
      commit: commit.change_id.clone(),
      message: description,
    };
    self.notice = None;
    load::run(cx, remote::mutate(request), move |this, outcome, cx| {
      let (ok, text) = mutation_message(&outcome, "Description updated");
      if !ok {
        for (change, description) in before {
          this.set_description(&change, &description);
        }
      }
      this.done(ok, text, "Failed to update description", None, cx);
    });
    cx.notify();
  }

  /// Sets the description of every loaded commit of change `change_id`;
  /// returns what each had.
  fn set_description(&mut self, change_id: &str, description: &str) -> Vec<(String, String)> {
    let Load::Ready(log) = &mut self.log else {
      return vec![];
    };
    log
      .commits
      .iter_mut()
      .filter(|c| c.change_id == change_id)
      .map(|c| {
        let old = std::mem::replace(&mut c.description, description.to_string());
        (c.change_id.clone(), old)
      })
      .collect()
  }

  fn confirm_undo(&mut self, commit: JjLogCommit, cx: &mut Context<Self>) {
    let this = cx.weak_entity();
    sheet::confirm(
      format!("Undo commit {}?", commit.short_id),
      format!(
        "\u{201c}{}\u{201d} is undone and its changes return to your working copy.",
        first_line(&commit)
      ),
      "Undo commit",
      false,
      move |_, cx| {
        let commit = commit.clone();
        this.update(cx, |screen, cx| screen.undo(commit, cx)).ok();
      },
      cx,
    );
  }

  fn undo(&mut self, commit: JjLogCommit, cx: &mut Context<Self>) {
    let request = TreqCommandRequest::UndoCommit {
      repo: self.target.repo.clone(),
      workspace: self.target.id.clone(),
      commit: commit.change_id.clone(),
    };
    let success = format!("Undid commit {}", commit.short_id);
    self.remove(
      commit.commit_id,
      request,
      move |this, outcome, cx| {
        let (ok, text) = mutation_message(&outcome, &success);
        this.done(ok, text, "Failed to undo commit", None, cx);
      },
      cx,
    );
  }

  fn confirm_delete(&mut self, commit: JjLogCommit, cx: &mut Context<Self>) {
    let this = cx.weak_entity();
    sheet::confirm(
      format!("Delete commit {}?", commit.short_id),
      format!(
        "\u{201c}{}\u{201d} is abandoned. You can undo it from the bar that follows.",
        first_line(&commit)
      ),
      "Delete commit",
      true,
      move |_, cx| {
        let commit = commit.clone();
        this.update(cx, |screen, cx| screen.delete(commit, cx)).ok();
      },
      cx,
    );
  }

  fn delete(&mut self, commit: JjLogCommit, cx: &mut Context<Self>) {
    let Target { repo, id, .. } = self.target.clone();
    let change = commit.change_id.clone();
    let key = self.keys.key_for("abandon", &[&repo, &id, &change]);
    let request = TreqCommandRequest::AbandonCommit {
      repo: repo.clone(),
      workspace: id.clone(),
      commit: change.clone(),
      idempotency_key: key,
    };
    let short = commit.short_id.clone();
    self.remove(
      commit.commit_id,
      request,
      move |this, outcome, cx| {
        this
          .keys
          .settle("abandon", &[&repo, &id, &change], &outcome);
        let undo = operation_of(&outcome).map(|op| (op, short.clone()));
        let (ok, text) = mutation_message(&outcome, &format!("Abandoned commit {short}"));
        this.done(ok, text, "Failed to delete commit", undo, cx);
      },
      cx,
    );
  }

  /// Hides commit `id` while `request` runs; a failure shows it again.
  fn remove(
    &mut self,
    id: String,
    request: TreqCommandRequest,
    then: impl FnOnce(&mut Self, Result<Mutation, String>, &mut Context<Self>) + 'static,
    cx: &mut Context<Self>,
  ) {
    self.removing.insert(id.clone());
    self.notice = None;
    load::run(cx, remote::mutate(request), move |this, outcome, cx| {
      if !matches!(outcome, Ok(Mutation::Applied(_))) {
        this.removing.remove(&id);
      }
      then(this, outcome, cx);
    });
    cx.notify();
  }

  fn undo_delete(&mut self, operation: String, short: String, cx: &mut Context<Self>) {
    let request = TreqCommandRequest::UndoOperation {
      repo: self.target.repo.clone(),
      workspace: Some(self.target.id.clone()),
      operation_id: operation,
    };
    self.notice = None;
    load::run(cx, remote::mutate(request), move |this, outcome, cx| {
      let (ok, text) = mutation_message(&outcome, &format!("Restored commit {short}"));
      this.done(ok, text, "Undo failed", None, cx);
    });
    cx.notify();
  }

  fn done(
    &mut self,
    ok: bool,
    text: String,
    failure: &str,
    undo: Option<(String, String)>,
    cx: &mut Context<Self>,
  ) {
    self.notice = Some(Notice {
      ok,
      text: if ok {
        text
      } else {
        format!("{failure}: {}", errors::describe(&text).message)
      },
      undo,
    });
    cx.emit(Changed);
    cx.notify();
  }
}

impl EventEmitter<Changed> for CommitsScreen {}

fn request(target: &Target) -> TreqCommandRequest {
  TreqCommandRequest::ListCommits {
    repo: target.repo.clone(),
    workspace: Some(target.id.clone()),
  }
}

/// Starts reading `target`'s commits before its tab is first shown.
pub fn prefetch(target: &Target) {
  if let Ok(endpoint) = crate::backend::get().endpoint() {
    remote::prefetch_on::<JjLogResult>(endpoint, request(target));
  }
}

impl Render for CommitsScreen {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let body = match &self.log {
      Load::Idle | Load::Loading => {
        widgets::card(cx).child(widgets::centered_message("Loading commits…", cx))
      }
      Load::Failed(error) => widgets::card(cx)
        .p_4()
        .child(widgets::error_text(error.clone(), cx)),
      Load::Ready(log) if log.commits.is_empty() => {
        widgets::card(cx).child(widgets::centered_message("No commits.", cx))
      }
      Load::Ready(log) => {
        let shown = visible(&log.commits, &self.removing);
        let rows: Vec<_> = (0..shown.len())
          .map(|ix| {
            let actions = actions(&shown, ix, &self.removing);
            let commit = shown[ix].clone();
            commit_row(&commit, cx)
              .id(SharedString::from(format!("commit-{}", commit.commit_id)))
              .when(!actions.is_empty(), |row| {
                row.active(|s| s.bg(cx.theme().accent)).on_click(
                  cx.listener(move |this, _, _, cx| this.menu(commit.clone(), actions.clone(), cx)),
                )
              })
          })
          .collect();
        widgets::card(cx).children(rows)
      }
    };
    let notice = self.notice.clone().map(|notice| {
      h_flex()
        .gap_3()
        .items_center()
        .justify_between()
        // Takes the row's free width and wraps, so a long error stays on
        // screen.
        .child(
          if notice.ok {
            widgets::muted(notice.text, cx)
          } else {
            widgets::error_text(notice.text, cx)
          }
          .flex_1()
          .min_w_0(),
        )
        .when_some(notice.undo, |bar, (operation, short)| {
          bar.child(
            Button::new("commit-undo-delete")
              .label("Undo")
              .small()
              .on_click(cx.listener(move |this, _, _, cx| {
                this.undo_delete(operation.clone(), short.clone(), cx)
              })),
          )
        })
    });
    div()
      .id("commits")
      .size_full()
      .overflow_y_scroll()
      .track_scroll(&self.scroll)
      .child(widgets::page().children(notice).child(body))
  }
}

fn commit_row(commit: &JjLogCommit, cx: &App) -> Stateful<Div> {
  let description = commit.description.lines().next().unwrap_or("").to_string();
  v_flex()
    .id("commit")
    .px_4()
    .py_3()
    .gap_1()
    .border_b_1()
    .border_color(cx.theme().border)
    .when(commit.on_target_only, |c| c.opacity(0.6))
    .child(
      h_flex()
        .gap_2()
        .items_center()
        .child(widgets::mono(commit.short_id.clone(), cx).text_color(cx.theme().primary))
        .when(commit.is_working_copy, |c| c.child(Tag::info().child("@")))
        .when(commit.has_conflicts, |c| {
          c.child(Tag::danger().child("conflict"))
        })
        .children(
          commit
            .bookmarks
            .iter()
            .map(|b| Tag::secondary().child(b.clone())),
        ),
    )
    .child(div().child(if description.is_empty() {
      "(no description)".to_string()
    } else {
      description
    }))
    .child(widgets::muted(
      format!(
        "{} · {} · +{} −{}",
        commit.author_name, commit.timestamp, commit.insertions, commit.deletions
      ),
      cx,
    ))
}

/// Editing a commit's description.
struct DescribeSheet {
  commit: JjLogCommit,
  screen: WeakEntity<CommitsScreen>,
  text: Entity<TextareaState>,
}

impl DescribeSheet {
  fn new(
    commit: JjLogCommit,
    screen: WeakEntity<CommitsScreen>,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Self {
    let initial = commit.description.clone();
    let text = cx.new(|cx| {
      TextareaState::new(window, cx)
        .auto_grow(3, 10)
        .placeholder("Describe this commit")
        .default_value(initial)
    });
    text.update(cx, |state, cx| state.focus(window, cx));
    Self {
      commit,
      screen,
      text,
    }
  }

  fn save(&mut self, cx: &mut Context<Self>) {
    let description = self.text.read(cx).value().to_string();
    let commit = self.commit.clone();
    sheet::close(cx);
    self
      .screen
      .update(cx, |screen, cx| {
        screen.save_description(&commit, description, cx)
      })
      .ok();
  }
}

impl Render for DescribeSheet {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    v_flex()
      .px_4()
      .pt_1()
      .gap_3()
      .child(Textarea::new(&self.text))
      .child(
        Button::new("describe-save")
          .primary()
          .large()
          .w_full()
          .label("Save")
          .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
      )
  }
}

#[cfg(test)]
mod tests {
  use std::collections::HashSet;

  use treq_lib::jj::JjLogCommit;

  use super::{actions, operation_of, visible, Action};
  use crate::remote::Mutation;

  fn commit(id: &str, on_target_only: bool) -> JjLogCommit {
    JjLogCommit {
      commit_id: id.into(),
      short_id: id.into(),
      change_id: format!("change-{id}"),
      description: format!("{id} message"),
      author_name: "a".into(),
      timestamp: "now".into(),
      parent_ids: vec![],
      is_working_copy: false,
      workspace_label: None,
      bookmarks: vec![],
      is_immutable: false,
      insertions: 0,
      deletions: 0,
      on_target_only,
      has_conflicts: false,
    }
  }

  #[test]
  fn only_the_first_own_commit_can_be_undone() {
    let log = [commit("a", false), commit("b", false), commit("t", true)];
    let none = HashSet::new();
    assert_eq!(
      actions(&log, 0, &none),
      vec![Action::EditDescription, Action::Undo, Action::Delete]
    );
    assert_eq!(
      actions(&log, 1, &none),
      vec![Action::EditDescription, Action::Delete]
    );
  }

  #[test]
  fn target_commits_have_no_actions() {
    let log = [commit("a", false), commit("t", true)];
    assert!(actions(&log, 1, &HashSet::new()).is_empty());
  }

  #[test]
  fn a_commit_being_removed_can_only_be_described() {
    let log = [commit("a", false)];
    let removing = HashSet::from(["a".to_string()]);
    assert_eq!(actions(&log, 0, &removing), vec![Action::EditDescription]);
  }

  #[test]
  fn removed_commits_are_hidden_and_the_next_one_becomes_first() {
    let log = [commit("a", false), commit("b", false)];
    let removing = HashSet::from(["a".to_string()]);
    let shown = visible(&log, &removing);
    assert_eq!(shown.len(), 1);
    assert_eq!(
      actions(&shown, 0, &removing),
      vec![Action::EditDescription, Action::Undo, Action::Delete]
    );
  }

  #[test]
  fn reads_the_operation_an_abandon_returns() {
    let applied = Ok(Mutation::Applied(serde_json::json!("op-1")));
    assert_eq!(operation_of(&applied), Some("op-1".to_string()));
    let replayed = Ok(Mutation::Applied(serde_json::Value::Null));
    assert_eq!(operation_of(&replayed), None);
  }
}
