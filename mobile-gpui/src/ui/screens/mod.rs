//! Pages below home: hosts, repositories, workspaces and their views.

pub mod add_host;
pub mod agent;
pub mod commits;
pub mod conflicts;
pub mod diff;
pub mod repos;
pub mod terminal;
pub mod workspace;
pub mod workspaces;

use gpui_kit::{App, Context};

use super::{model, nav};
use crate::store::SessionSnapshot;

/// Reopens the pages a snapshot names, after reconnecting.
pub fn restore_pages(snapshot: &SessionSnapshot, cx: &mut App) {
  nav::home(cx);
  repos::open(cx);
  if let Some(repo) = &snapshot.repo_path {
    workspaces::open(repo.clone(), cx);
    if let Some(workspace) = &snapshot.workspace {
      workspace::open(
        repo.clone(),
        workspace.id.clone(),
        workspace.name.clone(),
        cx,
      );
    }
  }
}

/// Calls `reload` whenever the app asks every screen to refetch (after a
/// reconnect or a return to the foreground).
pub fn on_epoch<V: 'static>(
  cx: &mut Context<V>,
  reload: impl Fn(&mut V, &mut Context<V>) + 'static,
) {
  let model = model(cx);
  let mut seen = model.read(cx).epoch;
  cx.observe(&model, move |view, model, cx| {
    let epoch = model.read(cx).epoch;
    if epoch != seen {
      seen = epoch;
      reload(view, cx);
    }
  })
  .detach();
}

/// The two-tap confirmation used for every remote mutation: the first tap
/// arms the action, the second runs it.
pub struct Confirm<A: PartialEq + Copy> {
  armed: Option<A>,
  pub running: Option<A>,
}

impl<A: PartialEq + Copy> Default for Confirm<A> {
  fn default() -> Self {
    Self {
      armed: None,
      running: None,
    }
  }
}

impl<A: PartialEq + Copy> Confirm<A> {
  /// Returns `true` when the tap should run `action` now.
  pub fn tap(&mut self, action: A) -> bool {
    if self.running.is_some() {
      return false;
    }
    if self.armed == Some(action) {
      self.armed = None;
      self.running = Some(action);
      true
    } else {
      self.armed = Some(action);
      false
    }
  }

  pub fn is_armed(&self, action: A) -> bool {
    self.armed == Some(action)
  }

  pub fn is_running(&self, action: A) -> bool {
    self.running == Some(action)
  }

  pub fn done(&mut self) {
    self.running = None;
  }

  pub fn disarm(&mut self) {
    self.armed = None;
  }
}

/// The message a mutation result leaves on screen.
pub fn mutation_message(
  outcome: &Result<crate::remote::Mutation, String>,
  success: &str,
) -> (bool, String) {
  match outcome {
    Ok(crate::remote::Mutation::Applied(_)) => (true, success.to_string()),
    Ok(crate::remote::Mutation::Ambiguous(reason)) => (
      false,
      format!("Could not confirm the change applied: {reason}"),
    ),
    Err(error) => (false, error.clone()),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn confirm_needs_two_taps_and_blocks_while_running() {
    let mut confirm = Confirm::default();
    assert!(!confirm.tap(1));
    assert!(confirm.is_armed(1));
    assert!(confirm.tap(1));
    assert!(confirm.is_running(1));
    assert!(!confirm.tap(2));
    confirm.done();
    assert!(!confirm.tap(2));
    // Tapping another action re-arms instead of running.
    assert!(!confirm.tap(1));
    assert!(confirm.is_armed(1));
  }
}
