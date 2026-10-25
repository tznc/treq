//! What a shell window has read from the backend, in place of Dashboard's
//! SWR caches: each repository's workspaces, their statuses, sessions and
//! branch, the supporting repositories, and the revisions screens read again
//! on.
//!
//! The window refreshes it on SWR's intervals ([`POLL`], [`SESSIONS_POLL`]),
//! when it gains focus, and on every change a screen reports (the
//! `NativeEvent`s React's `invalidateQueries` ran for), which also bump
//! [`Data::revision`], as `useCacheRevision` does.

use std::{
  collections::{HashMap, HashSet},
  time::Duration,
};

use crate::{
  core::{repo::RepoBranch, supporting_repos::SupportingRepo, WorkspaceSidebarStatus},
  github::{GitRemoteInfo, PrInfo},
  local_db::{Session, Workspace},
};

/// `workspaces` and `workspace-statuses`: `refreshInterval: 10000`.
pub const POLL: Duration = Duration::from_secs(10);
/// `sessions` and the linked repositories': `refreshInterval: 30000`.
pub const SESSIONS_POLL: Duration = Duration::from_secs(30);
/// In the background a window's polls read on every this many ticks, so
/// every 30 s and 90 s; back in front, it reads what they skipped at once.
pub const BACKGROUND_EVERY: u32 = 3;

/// What to read of a repository.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Read {
  pub workspaces: bool,
  pub sessions: bool,
  /// The git detail (`useWorkspaceGitDetail`), the branch and the remote,
  /// which only an invalidation reads again.
  pub detail: bool,
}

impl Read {
  pub const ALL: Read = Read {
    workspaces: true,
    sessions: true,
    detail: true,
  };
  pub const POLL: Read = Read {
    workspaces: true,
    sessions: false,
    detail: false,
  };
  pub const SESSIONS: Read = Read {
    workspaces: false,
    sessions: true,
    detail: false,
  };
}

/// One read of a repository; `None` where it was not read.
#[derive(Debug, Clone, Default)]
pub struct RepoRead {
  pub workspaces: Option<Vec<Workspace>>,
  pub statuses: Option<Vec<WorkspaceSidebarStatus>>,
  pub has_changes: Option<HashMap<i64, bool>>,
  pub sessions: Option<Vec<Session>>,
  pub branch: Option<Option<RepoBranch>>,
  pub default_branch: Option<Option<String>>,
  pub remote: Option<Option<GitRemoteInfo>>,
  /// Always read: the PR poller's cache.
  pub prs: HashMap<String, Option<PrInfo>>,
}

/// What the window knows of one repository.
#[derive(Debug, Clone, Default)]
pub struct RepoData {
  /// `None` until the first read returns.
  pub workspaces: Option<Vec<Workspace>>,
  pub statuses: Vec<WorkspaceSidebarStatus>,
  pub has_changes: HashMap<i64, bool>,
  pub sessions: Vec<Session>,
  pub branch: Option<RepoBranch>,
  pub default_branch: Option<String>,
  pub remote: Option<GitRemoteInfo>,
  /// Whether the remote was read yet (`useGitRemoteInfo`'s `isLoading`).
  pub remote_read: bool,
  pub prs: HashMap<String, Option<PrInfo>>,
  /// Workspaces being removed: their rows are gone before the removal
  /// finishes, and a read made meanwhile must not bring them back.
  pub hidden: HashSet<i64>,
}

/// The preview features the window's views depend on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Flags {
  pub linear: bool,
  pub logs: bool,
  pub remote_ssh: bool,
}

/// What the window has read.
#[derive(Debug, Clone, Default)]
pub struct Data {
  repos: HashMap<String, RepoData>,
  supporting: Vec<SupportingRepo>,
  /// Bumped on every invalidation, so screens read again.
  pub revision: u64,
  /// Bumped when a session list changes, so the terminal pane reads again.
  pub sessions_revision: u64,
  pub flags: Flags,
}

/// What a read changed, for the model to follow.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Applied {
  /// The workspace ids listed before, when the list was read again.
  pub before: Option<Vec<i64>>,
  pub changed: bool,
}

impl Data {
  pub fn repo(&self, path: &str) -> Option<&RepoData> {
    self.repos.get(path)
  }

  /// The repository's workspaces, as last read.
  pub fn workspaces(&self, path: &str) -> &[Workspace] {
    self
      .repos
      .get(path)
      .and_then(|r| r.workspaces.as_deref())
      .unwrap_or_default()
  }

  pub fn loaded(&self, path: &str) -> bool {
    self.repos.get(path).is_some_and(|r| r.workspaces.is_some())
  }

  pub fn sessions(&self, path: &str) -> &[Session] {
    self
      .repos
      .get(path)
      .map(|r| r.sessions.as_slice())
      .unwrap_or_default()
  }

  /// The main repository's supporting repositories.
  pub fn supporting(&self) -> &[SupportingRepo] {
    &self.supporting
  }

  pub fn set_supporting(&mut self, repos: Vec<SupportingRepo>) -> bool {
    if self.supporting == repos {
      return false;
    }
    self.supporting = repos;
    true
  }

  /// `currentBranch || repoDefaultBranch || "main"`.
  pub fn effective_default_branch(&self, path: &str) -> String {
    let repo = self.repos.get(path);
    repo
      .and_then(|r| r.branch.as_ref())
      .and_then(|b| b.current_branch.clone())
      .filter(|b| !b.is_empty())
      .or_else(|| repo.and_then(|r| r.default_branch.clone()))
      .filter(|b| !b.is_empty())
      .unwrap_or_else(|| "main".into())
  }

  /// `resolveHomeStackBranch`: `None` until the branch is known.
  pub fn home_stack_branch(&self, path: &str) -> Option<String> {
    let repo = self.repos.get(path)?;
    let branch = repo.branch.as_ref()?;
    branch
      .current_branch
      .clone()
      .filter(|b| !b.is_empty())
      .or_else(|| repo.default_branch.clone())
  }

  /// `homeRepoDisplayRef`: the home row's label.
  pub fn home_ref(&self, path: &str) -> Option<String> {
    self
      .repos
      .get(path)?
      .branch
      .as_ref()
      .map(|b| b.display_ref.clone())
  }

  /// Takes a read of repository `path`.
  pub fn apply(&mut self, path: &str, read: RepoRead) -> Applied {
    let repo = self.repos.entry(path.to_string()).or_default();
    let mut applied = Applied::default();
    if let Some(mut workspaces) = read.workspaces {
      let listed: HashSet<i64> = workspaces.iter().map(|w| w.id).collect();
      workspaces.retain(|w| !repo.hidden.contains(&w.id));
      // A read that no longer lists a removed workspace ends its hiding.
      repo.hidden.retain(|id| listed.contains(id));
      let ids = |list: &[Workspace]| list.iter().map(|w| w.id).collect::<Vec<_>>();
      let before = repo.workspaces.as_deref().map(ids);
      if repo.workspaces.as_ref() != Some(&workspaces) {
        applied.changed = true;
      }
      applied.before = Some(before.unwrap_or_default());
      repo.workspaces = Some(workspaces);
    }
    if let Some(mut statuses) = read.statuses {
      statuses.retain(|s| !repo.hidden.contains(&s.current.id));
      if repo.statuses != statuses {
        applied.changed = true;
      }
      repo.statuses = statuses;
    }
    if let Some(has_changes) = read.has_changes {
      applied.changed |= repo.has_changes != has_changes;
      repo.has_changes = has_changes;
    }
    if let Some(sessions) = read.sessions {
      if json(&repo.sessions) != json(&sessions) {
        applied.changed = true;
        self.sessions_revision += 1;
      }
      repo.sessions = sessions;
    }
    if let Some(branch) = read.branch {
      applied.changed |= repo.branch.as_ref().map(json) != branch.as_ref().map(json);
      repo.branch = branch;
    }
    if let Some(default_branch) = read.default_branch {
      applied.changed |= repo.default_branch != default_branch;
      repo.default_branch = default_branch;
    }
    if let Some(remote) = read.remote {
      applied.changed |= !repo.remote_read || repo.remote != remote;
      repo.remote = remote;
      repo.remote_read = true;
    }
    if repo.prs != read.prs {
      applied.changed = true;
      repo.prs = read.prs;
    }
    applied
  }

  /// Takes workspaces `ids` of `path` off the lists at once, and out of every
  /// read until one no longer lists them (a read made before the removal
  /// finished still does) or [`Data::unhide`].
  pub fn hide(&mut self, path: &str, ids: &[i64]) {
    let repo = self.repos.entry(path.to_string()).or_default();
    repo.hidden.extend(ids);
    if let Some(workspaces) = &mut repo.workspaces {
      workspaces.retain(|w| !ids.contains(&w.id));
    }
    repo.statuses.retain(|s| !ids.contains(&s.current.id));
    repo.has_changes.retain(|id, _| !ids.contains(id));
  }

  /// Ends [`Data::hide`] for a removal that failed: reads list `ids` again.
  pub fn unhide(&mut self, path: &str, ids: &[i64]) {
    if let Some(repo) = self.repos.get_mut(path) {
      for id in ids {
        repo.hidden.remove(id);
      }
    }
  }

  /// Forgets what was read of repositories the window no longer shows.
  pub fn retain(&mut self, paths: &[String]) {
    self.repos.retain(|path, _| paths.contains(path));
  }

  /// Forgets everything (Factory Reset's `clearSWRCache`).
  pub fn clear(&mut self) {
    self.repos.clear();
    self.supporting.clear();
    self.revision += 1;
  }
}

fn json<T: serde::Serialize + ?Sized>(value: &T) -> String {
  serde_json::to_string(value).unwrap_or_default()
}

#[cfg(test)]
mod tests {
  use super::{super::tree::tests::workspace, *};

  #[test]
  fn reports_what_a_read_changed() {
    let mut data = Data::default();
    let first = data.apply(
      "/repo",
      RepoRead {
        workspaces: Some(vec![workspace(1, "a", None)]),
        ..RepoRead::default()
      },
    );
    assert!(first.changed);
    assert_eq!(first.before, Some(vec![]));
    let again = data.apply(
      "/repo",
      RepoRead {
        workspaces: Some(vec![workspace(1, "a", None)]),
        ..RepoRead::default()
      },
    );
    assert!(!again.changed, "the same list changes nothing");
    assert_eq!(again.before, Some(vec![1]));
    assert_eq!(data.sessions_revision, 0);
    data.apply(
      "/repo",
      RepoRead {
        sessions: Some(vec![]),
        ..RepoRead::default()
      },
    );
    assert_eq!(data.sessions_revision, 0, "still no sessions");
  }

  #[test]
  fn a_hidden_workspace_stays_off_reads_until_one_drops_it() {
    let mut data = Data::default();
    let listed = |ids: &[i64]| RepoRead {
      workspaces: Some(
        ids
          .iter()
          .map(|&i| workspace(i, &format!("b{i}"), None))
          .collect(),
      ),
      ..RepoRead::default()
    };
    data.apply("/repo", listed(&[1, 2]));
    data.hide("/repo", &[2]);
    assert_eq!(data.workspaces("/repo").len(), 1, "gone at once");
    // A read made before the removal finished still lists it.
    let stale = data.apply("/repo", listed(&[1, 2]));
    assert!(!stale.changed, "the hidden row is not a change");
    assert_eq!(data.workspaces("/repo").len(), 1);
    // The read after it does not, which ends the hiding.
    data.apply("/repo", listed(&[1]));
    data.apply("/repo", listed(&[1, 2]));
    assert_eq!(
      data.workspaces("/repo").len(),
      2,
      "a new row with that id is shown"
    );
  }

  #[test]
  fn unhiding_brings_a_failed_removal_back() {
    let mut data = Data::default();
    let listed = RepoRead {
      workspaces: Some(vec![workspace(1, "a", None), workspace(2, "b", None)]),
      ..RepoRead::default()
    };
    data.apply("/repo", listed.clone());
    data.hide("/repo", &[2]);
    data.unhide("/repo", &[2]);
    data.apply("/repo", listed);
    assert_eq!(data.workspaces("/repo").len(), 2);
  }

  #[test]
  fn resolves_the_home_branch_as_dashboard_does() {
    let mut data = Data::default();
    assert_eq!(data.effective_default_branch("/repo"), "main");
    assert_eq!(data.home_stack_branch("/repo"), None, "not read yet");
    data.apply(
      "/repo",
      RepoRead {
        branch: Some(Some(RepoBranch {
          current_branch: None,
          display_ref: "abc1234".into(),
          is_detached: true,
        })),
        default_branch: Some(Some("trunk".into())),
        ..RepoRead::default()
      },
    );
    assert_eq!(data.effective_default_branch("/repo"), "trunk");
    assert_eq!(data.home_stack_branch("/repo").as_deref(), Some("trunk"));
    assert_eq!(data.home_ref("/repo").as_deref(), Some("abc1234"));
  }
}
