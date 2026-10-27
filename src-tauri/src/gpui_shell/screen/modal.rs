//! Dialogs `Dashboard` renders over whichever screen shows: the agent
//! prompt dialog, the prompt history and the stash list.
//!
//! React still owns whether one is open (and what the prompt dialog starts
//! with) and pushes it as a [`Modal`] with `gpui_modal_update`; GPUI reads
//! the history and stashes from the backend itself, applies and deletes
//! stashes, and hands back what only React can do: closing the dialog,
//! starting the agent session, running a past prompt, applying a stash to a
//! new workspace. Each dialog covers the whole window, sidebar included, as
//! React's `Dialog` did.

use std::sync::Arc;

use serde::Deserialize;
use tauri_plugin_gpui::gpui::{div, prelude::*, App, Div, SharedString, Stateful};

use super::{code_data::CodeHost, ActionSink, ScreenAction};
use crate::{
  gpui_shell::{
    native::{Backend, NativeEvent, ToastTone},
    popup::PopupSink,
    theme::Palette,
    ui::{self, TextSize},
  },
  jj::JjRevisionDiff,
  local_db::{PromptHistoryEntry, StashEntry},
};

pub mod agent_prompt;
pub mod prompt_history;
pub mod stash;

/// One dialog's data, tagged by `kind`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Modal {
  AgentPrompt(agent_prompt::Route),
  PromptHistory(prompt_history::Route),
  Stash(stash::Route),
}

/// What the dialogs read from and change in the backend; faked in tests.
/// Everything but `notify` blocks, so it is called off the main thread.
pub trait ModalsHost: CodeHost {
  /// Every prompt sent in the repository, most recent first.
  fn prompt_history(&self, repo_path: &str) -> Result<Vec<PromptHistoryEntry>, String>;
  /// Up to `limit` of them after the newest `offset`, most recent first.
  fn prompt_history_page(
    &self,
    repo_path: &str,
    offset: usize,
    limit: usize,
  ) -> Result<Vec<PromptHistoryEntry>, String> {
    self
      .prompt_history(repo_path)
      .map(|all| all.into_iter().skip(offset).take(limit).collect())
  }
  fn stashes(&self, repo_path: &str) -> Result<Vec<StashEntry>, String>;
  fn stash_diff(&self, repo_path: &str, stash_id: i64) -> Result<JjRevisionDiff, String>;
  /// Copies the stash onto `target_branch` (`.` for the home repo).
  fn apply_stash(&self, repo_path: &str, stash_id: i64, target_branch: &str) -> Result<(), String>;
  fn delete_stash(&self, repo_path: &str, stash_id: i64) -> Result<(), String>;
  fn stash_patch(&self, repo_path: &str, stash_id: i64) -> Result<String, String>;
}

impl ModalsHost for Backend {
  fn prompt_history(&self, repo_path: &str) -> Result<Vec<PromptHistoryEntry>, String> {
    crate::core::sessions::prompt_history(&self.state_scope(repo_path))
  }

  fn prompt_history_page(
    &self,
    repo_path: &str,
    offset: usize,
    limit: usize,
  ) -> Result<Vec<PromptHistoryEntry>, String> {
    crate::core::sessions::prompt_history_page(&self.state_scope(repo_path), limit, offset)
  }

  fn stashes(&self, repo_path: &str) -> Result<Vec<StashEntry>, String> {
    self.local_only(repo_path, "Stashing")?;
    crate::core::list_stashes(repo_path)
  }

  fn stash_diff(&self, repo_path: &str, stash_id: i64) -> Result<JjRevisionDiff, String> {
    self.local_only(repo_path, "Stashing")?;
    crate::core::get_stash_diff(repo_path, stash_id)
  }

  fn apply_stash(&self, repo_path: &str, stash_id: i64, target_branch: &str) -> Result<(), String> {
    self.local_only(repo_path, "Stashing")?;
    crate::core::apply_stash(repo_path, stash_id, target_branch)
  }

  fn delete_stash(&self, repo_path: &str, stash_id: i64) -> Result<(), String> {
    self.local_only(repo_path, "Stashing")?;
    crate::core::delete_stash(repo_path, stash_id)
  }

  fn stash_patch(&self, repo_path: &str, stash_id: i64) -> Result<String, String> {
    self.local_only(repo_path, "Stashing")?;
    crate::core::export_stash_git_patch(repo_path, stash_id)
  }
}

/// Everything a dialog view needs from its screen.
#[derive(Clone)]
pub struct Env {
  pub host: Arc<dyn ModalsHost>,
  /// Where actions for React go.
  pub react: ActionSink,
  /// Where tooltips go.
  pub popups: PopupSink,
}

impl Env {
  /// Sends `action` to React, never inside the dialog's own update: React's
  /// sink may route back here.
  pub fn send(&self, action: ScreenAction, cx: &mut App) {
    let react = self.react.clone();
    cx.defer(move |cx| react(action, cx));
  }

  pub fn press(&self, id: &str, cx: &mut App) {
    self.send(ScreenAction::Press { id: id.into() }, cx);
  }

  pub fn field(&self, field: &str, value: serde_json::Value, cx: &mut App) {
    self.send(
      ScreenAction::Field {
        field: field.into(),
        value,
      },
      cx,
    );
  }

  /// Tells React what the dialog changed.
  pub fn notify(&self, event: NativeEvent) {
    super::workspace_data::WorkspaceHost::notify(self.host.as_ref(), event);
  }

  pub fn toast(&self, tone: ToastTone, title: &str, description: impl Into<String>) {
    self.notify(NativeEvent::Toast {
      title: title.into(),
      description: description.into(),
      tone,
      action: None,
    });
  }

  /// Tells React the repository's state changed (a stash was applied).
  pub fn workspace_changed(&self, repo_path: &str) {
    self.notify(NativeEvent::WorkspaceChanged {
      repo_path: repo_path.into(),
    });
  }
}

/// `text-sm text-muted-foreground py-8 text-center`.
pub fn message(id: &'static str, text: impl Into<SharedString>, p: &Palette) -> Stateful<Div> {
  ui::text(div(), TextSize::Sm)
    .id(id)
    .debug_selector(move || id.into())
    .py_8()
    .text_center()
    .text_color(p.muted_foreground)
    .child(text.into())
}

/// `a` contains `b`'s letters in order, ignoring case (cmdk's filter).
pub fn fuzzy_matches(query: &str, text: &str) -> bool {
  let mut text = text.chars().flat_map(char::to_lowercase);
  query
    .chars()
    .flat_map(char::to_lowercase)
    .filter(|c| !c.is_whitespace())
    .all(|c| text.any(|t| t == c))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn fuzzy_matching_takes_letters_in_order() {
    assert!(fuzzy_matches("", "anything"));
    assert!(fuzzy_matches("fix", "treq/fix-login"));
    assert!(fuzzy_matches("TFL", "treq/fix-login"));
    assert!(!fuzzy_matches("xf", "fix"));
    assert!(!fuzzy_matches("zzz", "main"));
  }
}
