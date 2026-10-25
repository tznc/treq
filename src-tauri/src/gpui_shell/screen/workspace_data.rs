//! What the workspace header reads from the backend, and how
//! `ShowWorkspace` turns it into the header and tab row.

use std::collections::HashSet;

use serde::Deserialize;

use super::{
  checklist::ToolsHost,
  workspace::{
    Checks, CommitsLabel, CreatePr, Disabled, Header, HomeTarget, Linear, Loc, Merge, Pill,
    PrState, Rebase, Sync, Tab, TabRow, Target, Tone, Tooltip, ViewPr,
  },
};
use crate::{
  core::{
    self,
    feature_preview::{self, PreviewFeature},
    RemoteSyncStatus, WorkspaceStatus,
  },
  github::{GitRemoteInfo, PrCiStatus, PrInfo},
  gpui_shell::{
    native::{Backend, NativeEvent},
    popover::data::{read_checks, ChecksData},
    remote_repo::{unsupported, Remote},
  },
  jj::{HomeRebaseDryRunResult, JjFileChange, JjLogResult},
  local_db,
};

/// `ShowWorkspace`'s `defaultBranch`: the literal it compares branches with.
pub const DEFAULT_BRANCH: &str = "main";

/// What the window model presents: which workspace shows, and the state of
/// the flows it runs (target branch, Changes refresh, PR creation, checks).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Route {
  pub repo_path: String,
  /// `None` for the home repository.
  pub workspace_id: Option<i64>,
  /// The home repository's checked-out branch (`mainRepoBranch`).
  pub home_branch: String,
  pub tab: Tab,
  /// The target branch is being changed (`rebasing`).
  pub retargeting: bool,
  pub creating_pr: bool,
  /// Workflows started from this window that have not reported yet.
  pub running_checks: Vec<String>,
  /// Workflow files running as a whole (`isRunning(filename)`), where
  /// `running_checks` also holds those running a single job.
  #[serde(default)]
  pub running_workflows: Vec<String>,
  /// The workspace's setup script is being re-run from this window.
  #[serde(default)]
  pub setup_rerunning: bool,
  /// Bumped whenever the window model refreshes its data, so GPUI reads
  /// again.
  pub revision: u64,
  /// The commit the Commits tab should scroll to and open.
  #[serde(default)]
  pub scroll_to_commit: Option<String>,
  /// `showCommittedChanges`, when the Changes tab offers the toggle.
  #[serde(default)]
  pub show_committed: Option<bool>,
  /// Where the Changes tab can move files (`useChangeFilesMove`).
  #[serde(default)]
  pub move_targets: Vec<super::changes::MoveTarget>,
  /// Bumped when the workspace's files change or the window gains focus.
  #[serde(default)]
  pub changes_revision: u64,
  /// The Logs tab's source (`LogSource`) while it shows.
  #[serde(default)]
  pub logs_source: Option<String>,
  /// Bumped by "open in Logs" links, so the tab starts over on `logs_source`.
  #[serde(default)]
  pub logs_key: u64,
  /// The terminal pane below the tab.
  #[serde(default)]
  pub terminal: Option<super::terminal::Route>,
  /// GPUI draws the Code tab's file browser (`showFileBrowserInCode`).
  #[serde(default)]
  pub file_browser: Option<super::file_browser::Route>,
  /// A remote repository's strip, banner and notice above the header;
  /// absent for a local one. (`remote` is the snapshot's remote dialogs.)
  #[serde(default)]
  pub remote_chrome: Option<super::remote_chrome::RemoteChrome>,
  /// The workspace as the window last listed it, so the tabs start on it
  /// before the header's own read lands.
  #[serde(default)]
  pub workspace: Option<local_db::Workspace>,
}

impl Route {
  /// Which workspace the loaded data is for.
  pub fn key(&self) -> (String, Option<i64>, String) {
    (
      self.repo_path.clone(),
      self.workspace_id,
      self.home_branch.clone(),
    )
  }

  /// `defaultTargetBranch`.
  pub(super) fn default_target(&self) -> &str {
    if self.home_branch.is_empty() {
      DEFAULT_BRANCH
    } else {
      &self.home_branch
    }
  }
}

/// Everything the header shows, read in one go off the main thread.
#[derive(Clone, Default)]
pub struct Loaded {
  /// `None` for the home repository.
  pub workspace: Option<local_db::Workspace>,
  pub status: Option<WorkspaceStatus>,
  pub remote: Option<GitRemoteInfo>,
  /// The workspace's commits, without target history.
  pub commits: Option<JjLogResult>,
  /// Unique paths changed in the workspace, for the Changes pill.
  pub review_count: usize,
  pub uncommitted_count: usize,
  /// Commits on the target branch the home branch lacks.
  pub home_ahead: usize,
  pub home_dry_run: Option<HomeRebaseDryRunResult>,
  pub checks: Option<ChecksData>,
  pub linear_enabled: bool,
  pub logs_enabled: bool,
}

/// The branch's pull request and its checks, from the PR poller's cache.
/// The outer `None` is "not polled yet".
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PrCache {
  pub pr: Option<Option<PrInfo>>,
  pub ci: Option<PrCiStatus>,
}

/// What a header action reports, for its toast.
pub struct Outcome {
  pub ok: bool,
  pub warning: bool,
  pub title: String,
  pub description: String,
}

/// What the workspace header asks of the backend; faked in tests. All but
/// `notify` and `open_url` block, so they are called off the main thread.
pub trait WorkspaceHost: ToolsHost + super::launch_data::LaunchHost {
  fn load(&self, route: &Route) -> Loaded;
  /// Also starts polling the repository's pull requests.
  fn pr_cache(&self, repo_path: &str, branch: &str) -> PrCache;
  fn push(&self, repo_path: &str, workspace_id: Option<i64>) -> Outcome;
  fn sync(&self, repo_path: &str, workspace_id: Option<i64>) -> Outcome;
  fn rebase_home(&self, repo_path: &str, branch: &str) -> Outcome;
  /// Fetches, then resets the workspace's branch to `origin`.
  fn reset_to_remote(&self, repo_path: &str, workspace_id: i64) -> Outcome;
  fn notify(&self, event: NativeEvent);
}

fn unique_paths<'a>(paths: impl Iterator<Item = &'a str>) -> usize {
  paths.collect::<HashSet<_>>().len()
}

/// The Changes pill's counts: unique paths changed (committed too, off
/// the home branch) and those uncommitted.
fn changed_counts(
  repo: &str,
  route: &Route,
  workspace: Option<&local_db::Workspace>,
) -> (usize, usize) {
  let warn = |what: &str, err: String| log::warn!("workspace header: {what}: {err}");
  let include_committed = workspace.is_some_and(|w| w.branch_name != route.default_target());
  match route.workspace_id {
    Some(id) if include_committed => core::workspace_diff(repo, id)
      .map(|diff| {
        let all = diff.uncommitted_files.iter().chain(&diff.committed_files);
        (
          unique_paths(all.map(|f| f.path.as_str())),
          diff.uncommitted_files.len(),
        )
      })
      .unwrap_or_else(|e| {
        warn("diff", e);
        (0, 0)
      }),
    id => core::list_changed_files(repo, id)
      .map(|files| {
        let count = unique_paths(files.iter().map(|f| f.path.as_str()));
        (count, count)
      })
      .unwrap_or_else(|e| {
        warn("changed files", e);
        (0, 0)
      }),
  }
}

/// The home branch's commits behind its target, and the dry-run rebase
/// onto it, on a home feature branch.
fn home_rebase(repo: &str, route: &Route) -> (usize, Option<HomeRebaseDryRunResult>) {
  let home_branch = route.home_branch.as_str();
  let on_home_feature = route.workspace_id.is_none() && home_branch != DEFAULT_BRANCH;
  let home_ahead = if on_home_feature {
    core::list_commits(repo, None, true, None, Some(14))
      .map(|log| log.commits.iter().filter(|c| c.on_target_only).count())
      .unwrap_or(0)
  } else {
    0
  };
  let home_dry_run = (on_home_feature && home_ahead > 0 && !home_branch.is_empty())
    .then(|| core::repo::dry_run_home_rebase(repo, home_branch, DEFAULT_BRANCH))
    .flatten();
  (home_ahead, home_dry_run)
}

impl WorkspaceHost for Backend {
  fn load(&self, route: &Route) -> Loaded {
    if let Some(remote) = self.remote(&route.repo_path) {
      return load_remote(self, &remote, route);
    }
    let repo = route.repo_path.as_str();
    let warn = |what: &str, err: String| log::warn!("workspace header: {what}: {err}");
    // The reads are independent but for the counts, which need the
    // workspace: run them side by side, so the header waits for the
    // slowest rather than for all of them in turn.
    std::thread::scope(|scope| {
      let status = scope.spawn(|| {
        core::workspace_status(repo, route.workspace_id)
          .map_err(|e| warn("status", e))
          .ok()
      });
      let remote = scope.spawn(|| {
        crate::core::github_cli::remote(repo)
          .map_err(|e| warn("remote", e))
          .ok()
          .flatten()
      });
      let commits = scope.spawn(|| {
        route.workspace_id.and_then(|id| {
          core::list_commits(repo, Some(id), false, None, None)
            .map_err(|e| warn("commits", e))
            .ok()
        })
      });
      let home = scope.spawn(|| home_rebase(repo, route));
      let checks = scope.spawn(|| route.workspace_id.map(|id| read_checks(repo, id)));
      let workspace = route.workspace_id.and_then(|id| {
        core::list_workspaces(repo)
          .map_err(|e| warn("workspaces", e))
          .ok()?
          .into_iter()
          .find(|w| w.id == id)
      });
      let (review_count, uncommitted_count) = changed_counts(repo, route, workspace.as_ref());
      let (linear_enabled, logs_enabled) = self.with_db(|db| {
        (
          feature_preview::is_enabled(db, PreviewFeature::LinearIntegration),
          feature_preview::is_enabled(db, PreviewFeature::Logs),
        )
      });
      let (home_ahead, home_dry_run) = super::joined(home);
      Loaded {
        workspace,
        status: super::joined(status),
        remote: super::joined(remote),
        commits: super::joined(commits),
        review_count,
        uncommitted_count,
        home_ahead,
        home_dry_run,
        checks: super::joined(checks),
        linear_enabled,
        logs_enabled,
      }
    })
  }

  fn pr_cache(&self, repo_path: &str, branch: &str) -> PrCache {
    if self.remote_containing(repo_path).is_some() {
      // `localReadOr`: no pull request is read for a remote repository.
      return PrCache {
        pr: Some(None),
        ci: None,
      };
    }
    let poller = crate::pr_status::global();
    poller.watch_repo(repo_path);
    PrCache {
      pr: poller.get_cached(repo_path, branch),
      ci: poller.get_cached_ci(repo_path, branch).flatten(),
    }
  }

  fn push(&self, repo_path: &str, workspace_id: Option<i64>) -> Outcome {
    let pushed = match self.remote(repo_path) {
      Some(remote) => remote.push(workspace_id),
      None => core::push_workspace_to_remote(repo_path, workspace_id),
    };
    match pushed {
      Ok(_) => Outcome::ok("Pushed to remote", "Changes pushed".to_string()),
      Err(err) => Outcome::failed("Push failed", err),
    }
  }

  fn sync(&self, repo_path: &str, workspace_id: Option<i64>) -> Outcome {
    let synced = match self.remote(repo_path) {
      Some(remote) => remote
        .pull(workspace_id)
        .and_then(|pull| remote.push(workspace_id).map(|_| pull)),
      None => {
        let style = self.with_db(core::resolve_conflict_marker_style_from_db);
        core::pull_workspace_from_remote(repo_path, workspace_id, &style)
          .and_then(|pull| core::push_workspace_to_remote(repo_path, workspace_id).map(|_| pull))
      }
    };
    match synced {
      Ok(pull) if pull.has_conflicts => Outcome {
        ok: true,
        warning: true,
        title: "Synced with conflicts".into(),
        description: "Fetched and pushed; resolve remaining conflicts locally".into(),
      },
      Ok(_) => Outcome::ok(
        "Synced with remote",
        "Fetched and pushed changes".to_string(),
      ),
      Err(err) => Outcome::failed("Sync failed", err),
    }
  }

  fn rebase_home(&self, repo_path: &str, branch: &str) -> Outcome {
    if self.remote_containing(repo_path).is_some() {
      return Outcome::failed(
        "Rebase failed",
        unsupported("Rebasing the repository branch"),
      );
    }
    match core::repo::rebase_home_branch(repo_path, branch, DEFAULT_BRANCH) {
      Ok(result) if result.success => Outcome::ok(
        "Rebase complete",
        if result.message.is_empty() {
          "Branch rebased onto target".to_string()
        } else {
          result.message
        },
      ),
      Ok(result) => Outcome::failed("Rebase failed", result.message),
      Err(err) => Outcome::failed("Rebase failed", err),
    }
  }

  fn reset_to_remote(&self, repo_path: &str, workspace_id: i64) -> Outcome {
    if self.remote_containing(repo_path).is_some() {
      return Outcome::failed("Reset failed", unsupported("Resetting to the remote"));
    }
    match core::reset_workspace_to_remote(repo_path, workspace_id) {
      Ok(dropped) => Outcome::ok(
        "Reset to remote",
        format!("Dropped {}", plural(dropped, "local commit")),
      ),
      Err(err) => Outcome::failed("Reset failed", err),
    }
  }

  fn notify(&self, event: NativeEvent) {
    Backend::notify(self, event);
  }
}

/// What the header reads of a remote repository.
struct RemoteHeader {
  workspace: Option<local_db::Workspace>,
  status: Option<WorkspaceStatus>,
  origin: Option<GitRemoteInfo>,
  commits: Option<JjLogResult>,
  review_count: usize,
  uncommitted_count: usize,
  home_ahead: usize,
}

/// The header's reads, side by side: each is a round trip to the host, so
/// the header waits for the slowest rather than for all of them in turn.
/// The workspace's diff, which tells committed from uncommitted files, is
/// started before its branch is known, as it is wanted for any workspace
/// off the default branch.
fn read_remote_header(remote: &Remote, route: &Route) -> RemoteHeader {
  let warn = |what: &str, err: String| log::warn!("remote workspace header: {what}: {err}");
  let on_home_feature = route.workspace_id.is_none() && route.home_branch != DEFAULT_BRANCH;
  let (workspace, status, origin, commits, diff, home_files, home_ahead) =
    std::thread::scope(|scope| {
      let workspace = scope.spawn(|| {
        route.workspace_id.and_then(|id| {
          remote
            .workspaces()
            .map_err(|e| warn("workspaces", e))
            .ok()?
            .into_iter()
            .find(|w| w.id == id)
        })
      });
      let status = scope.spawn(|| {
        remote
          .status(route.workspace_id)
          .map_err(|e| warn("status", e))
          .ok()
      });
      let origin = scope.spawn(|| {
        remote
          .git_remote()
          .map_err(|e| warn("remote", e))
          .ok()
          .flatten()
      });
      let commits = scope.spawn(|| {
        route.workspace_id.and_then(|id| {
          remote
            .commits(Some(id))
            .map_err(|e| warn("commits", e))
            .ok()
        })
      });
      let diff = scope.spawn(|| route.workspace_id.map(|id| remote.workspace_diff(id)));
      let home_files = scope.spawn(|| {
        route
          .workspace_id
          .is_none()
          .then(|| remote.changed_files(None))
      });
      let home_ahead = scope.spawn(|| {
        if on_home_feature {
          remote
            .commits(None)
            .map(|log| log.commits.iter().filter(|c| c.on_target_only).count())
            .unwrap_or(0)
        } else {
          0
        }
      });
      (
        super::joined(workspace),
        super::joined(status),
        super::joined(origin),
        super::joined(commits),
        super::joined(diff),
        super::joined(home_files),
        super::joined(home_ahead),
      )
    });
  let include_committed = workspace
    .as_ref()
    .is_some_and(|w| w.branch_name != route.default_target());
  let changed = |files: Result<Vec<JjFileChange>, String>| {
    files
      .map(|files| {
        let count = unique_paths(files.iter().map(|f| f.path.as_str()));
        (count, count)
      })
      .unwrap_or_else(|e| {
        warn("changed files", e);
        (0, 0)
      })
  };
  let (review_count, uncommitted_count) = match (route.workspace_id, diff) {
    (Some(_), Some(diff)) if include_committed => diff
      .map(|diff| {
        let all = diff.uncommitted_files.iter().chain(&diff.committed_files);
        (
          unique_paths(all.map(|f| f.path.as_str())),
          diff.uncommitted_files.len(),
        )
      })
      .unwrap_or_else(|e| {
        warn("diff", e);
        (0, 0)
      }),
    (None, _) => changed(home_files.unwrap_or_else(|| remote.changed_files(None))),
    (id, _) => changed(remote.changed_files(id)),
  };
  RemoteHeader {
    workspace,
    status,
    origin,
    commits,
    review_count,
    uncommitted_count,
    home_ahead,
  }
}

/// `ShowWorkspace`'s reads for a remote repository, through its typed
/// commands. The home branch's dry-run rebase and the workflow checks are
/// local-only (`assertLocalOperation`), so they stay empty.
fn load_remote(backend: &Backend, remote: &Remote, route: &Route) -> Loaded {
  let header = read_remote_header(remote, route);
  let (linear_enabled, logs_enabled) = backend.with_db(|db| {
    (
      feature_preview::is_enabled(db, PreviewFeature::LinearIntegration),
      feature_preview::is_enabled(db, PreviewFeature::Logs),
    )
  });
  Loaded {
    workspace: header.workspace,
    status: header.status,
    remote: header.origin,
    commits: header.commits,
    review_count: header.review_count,
    uncommitted_count: header.uncommitted_count,
    home_ahead: header.home_ahead,
    home_dry_run: None,
    checks: None,
    linear_enabled,
    logs_enabled,
  }
}

impl Outcome {
  pub(super) fn ok(title: &str, description: String) -> Self {
    Self {
      ok: true,
      warning: false,
      title: title.into(),
      description,
    }
  }

  pub(super) fn failed(title: &str, description: String) -> Self {
    Self {
      ok: false,
      warning: false,
      title: title.into(),
      description,
    }
  }
}

/// Header actions in flight (`actionPending`, `homeRebasing`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pending {
  #[default]
  None,
  Push,
  Sync,
  HomeRebase,
  Reset,
}

/// The workspace row's `metadata` JSON.
#[derive(Debug, Default, Deserialize)]
struct Metadata {
  linear_issue_key: Option<String>,
  linear_issue_url: Option<String>,
  linear_issue_title: Option<String>,
}

fn plural(n: usize, word: &str) -> String {
  format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

/// The branch must be pushed before a pull request can be made for it:
/// it is not on the remote, or has commits the remote lacks.
pub fn needs_push(loaded: &Loaded) -> bool {
  loaded.workspace.as_ref().is_some_and(|w| w.not_on_remote)
    || matches!(
      loaded.status.as_ref().map(|s| &s.remote_sync),
      Some(RemoteSyncStatus::Ahead { .. } | RemoteSyncStatus::Diverged { .. })
    )
}

/// `ShowWorkspace`'s header and tab row, from what was loaded. `None` until
/// the first load, when React draws nothing different.
pub fn workspace_view(
  route: &Route,
  loaded: &Loaded,
  pr: &PrCache,
  pending: Pending,
) -> (Header, Option<TabRow>) {
  let workspace = loaded.workspace.as_ref();
  let status = loaded.status.as_ref();
  let busy = pending != Pending::None;
  let metadata: Metadata = workspace
    .and_then(|w| w.metadata.as_deref())
    .and_then(|m| serde_json::from_str(m).ok())
    .unwrap_or_default();
  let branch = workspace
    .map(|w| w.branch_name.clone())
    .filter(|b| !b.is_empty())
    .unwrap_or_else(|| route.default_target().to_string());
  let on_feature = workspace.is_some_and(|w| w.branch_name != DEFAULT_BRANCH);
  let not_on_remote = workspace.is_some_and(|w| w.not_on_remote);

  let conflicts: HashSet<&str> = status
    .map(|s| {
      s.conflicted_files
        .iter()
        .map(|f| f.trim())
        .filter(|f| !f.is_empty())
        .collect()
    })
    .unwrap_or_default();
  let conflict_count = conflicts.len();
  let merge_blocked = route.retargeting || conflict_count > 0;
  let (ahead, behind) = match status.map(|s| &s.remote_sync) {
    Some(RemoteSyncStatus::Ahead { count }) => (*count, 0),
    Some(RemoteSyncStatus::Behind { count }) => (0, *count),
    Some(RemoteSyncStatus::Diverged { ahead, behind }) => (*ahead, *behind),
    _ => (0, 0),
  };
  let ci = pr.ci.as_ref().filter(|ci| ci.total > 0);
  let ci_passing = ci.is_some_and(|ci| ci.state == "success");
  let pr_info = pr.pr.clone().flatten();
  let pr_loaded = pr.pr.is_some();
  let has_remote = loaded.remote.is_some();
  let commits_ahead = status.map_or(0, |s| s.commits_ahead_of_target.len());
  let has_workspace_commits = loaded.commits.as_ref().is_some_and(|log| {
    log
      .commits
      .iter()
      .any(|c| !c.on_target_only && !c.is_working_copy)
  });

  let home_target = (workspace.is_none() && branch != DEFAULT_BRANCH).then(|| HomeTarget {
    branch: DEFAULT_BRANCH.into(),
    rebase: (loaded.home_ahead > 0).then(|| Rebase {
      tooltip: if pending == Pending::HomeRebase {
        "Rebasing...".into()
      } else {
        format!(
          "Rebase {branch} onto {DEFAULT_BRANCH} ({})",
          plural(loaded.home_ahead, "new commit")
        )
      },
      rebasing: pending == Pending::HomeRebase,
      conflicts: loaded
        .home_dry_run
        .as_ref()
        .filter(|d| d.would_conflict)
        .map(|d| d.conflicted_files.clone()),
    }),
  });

  let needs_push = needs_push(loaded);
  let has_commits = has_workspace_commits || commits_ahead > 0;
  const NO_COMMITS: &str =
    "Make a commit before creating a pull request. Uncommitted working-copy changes don't count.";
  // An open or merged PR already covers the branch; a closed one may be
  // replaced.
  let create_pr = (on_feature
    && has_remote
    && pr_loaded
    && pr_info
      .as_ref()
      .is_none_or(|p| p.state != "OPEN" && p.state != "MERGED"))
  .then(|| CreatePr {
    label: if route.creating_pr {
      if needs_push {
        "Pushing & creating…"
      } else {
        "Creating…"
      }
    } else {
      "Create PR"
    }
    .into(),
    creating: route.creating_pr,
    disabled: route.creating_pr || !has_commits,
    tooltip: if !has_commits {
      NO_COMMITS
    } else if needs_push {
      "Push this branch and create a pull request on GitHub"
    } else {
      "Create a pull request on GitHub"
    }
    .into(),
    menu_tooltip: if has_commits {
      "More pull request options"
    } else {
      NO_COMMITS
    }
    .into(),
  });

  let checks = loaded.checks.as_ref().and_then(|summary| {
    let (state, passed, total) = summary.rollup(&route.running_checks);
    let has_treq = state != "none";
    if !has_treq && ci.is_none() {
      return None;
    }
    // Failed beats running beats passed; nothing run yet shows no status.
    let mut states: Vec<Option<&str>> = Vec::new();
    match state {
      "passed" | "failed" | "running" => states.push(Some(state)),
      "not-run" => states.push(None),
      _ => {}
    }
    if let Some(ci) = ci {
      states.push(Some(match ci.state.as_str() {
        "success" => "passed",
        "failure" => "failed",
        _ => "running",
      }));
    }
    let combined = if states.contains(&Some("failed")) {
      Some("failed")
    } else if states.contains(&Some("running")) {
      Some("running")
    } else if !states.is_empty() && states.iter().all(|s| *s == Some("passed")) {
      Some("passed")
    } else {
      None
    };
    let (ci_passed, ci_total) = ci.map_or((0, 0), |ci| (ci.passed as usize, ci.total as usize));
    let parts: Vec<String> = [
      has_treq.then(|| match state {
        "running" => "Treq running".to_string(),
        "not-run" => "Treq not run".to_string(),
        _ => format!("Treq {passed}/{total}"),
      }),
      ci.map(|ci| format!("GitHub {}/{}", ci.passed, ci.total)),
    ]
    .into_iter()
    .flatten()
    .collect();
    Some(Checks {
      status: combined.map(str::to_string),
      count: format!("{}/{}", passed + ci_passed, total + ci_total),
      name: format!("Checks: {}", parts.join(", ")),
    })
  });

  let header = Header {
    branch: branch.clone(),
    home: workspace.is_none(),
    linear: match (&metadata.linear_issue_key, &metadata.linear_issue_url) {
      (Some(key), Some(url)) if workspace.is_some() && loaded.linear_enabled => Some(Linear {
        key: key.clone(),
        url: url.clone(),
        title: metadata.linear_issue_title.clone(),
      }),
      _ => None,
    },
    target: on_feature.then(|| Target {
      branch: Some(
        workspace
          .and_then(|w| w.target_branch.clone())
          .unwrap_or_else(|| route.default_target().to_string()),
      ),
      loading: false,
      disabled: route.retargeting,
    }),
    home_target,
    stack: workspace.is_none().then(|| Tooltip {
      tooltip: format!("Create stacked workspace from {branch}"),
    }),
    push: workspace
      .filter(|w| w.not_on_remote && (!has_remote || w.branch_name == DEFAULT_BRANCH))
      .map(|_| Disabled { disabled: busy }),
    view_pr: (on_feature && has_remote)
      .then_some(pr_info.as_ref())
      .flatten()
      .map(|p| ViewPr {
        number: p.number,
        state: match p.state.as_str() {
          "CLOSED" => PrState::Closed,
          "MERGED" => PrState::Merged,
          _ => PrState::Open,
        },
        draft: p.is_draft,
      }),
    create_pr,
    status_loading: status.is_none(),
    sync: (!not_on_remote && (ahead > 0 || behind > 0)).then_some(Sync {
      ahead: ahead as u32,
      behind: behind as u32,
      disabled: busy,
      spinning: matches!(pending, Pending::Sync | Pending::Reset),
    }),
    checks,
    merge: on_feature.then(|| Merge {
      passing: ci_passing,
      disabled: merge_blocked,
      tooltip: merge_blocked.then(|| {
        if route.retargeting {
          "Rebasing in progress...".to_string()
        } else {
          format!(
            "Cannot merge: {} detected",
            plural(conflict_count, "conflict")
          )
        }
      }),
    }),
    details: workspace.is_some(),
    more: workspace.map(|_| Disabled { disabled: busy }),
  };

  let has_conflict = conflict_count > 0 || status.is_some_and(|s| s.partial.has_conflicts);
  let tab_row = Some({
    let review = loaded.review_count;
    let has_uncommitted =
      loaded.uncommitted_count > 0 || status.is_some_and(|s| s.partial.has_changes);
    TabRow {
      tab: route.tab,
      commits: if workspace.is_none() {
        has_conflict.then_some(CommitsLabel::HomeConflict)
      } else {
        (commits_ahead > 0).then_some(CommitsLabel::Count {
          count: commits_ahead as u32,
          tone: if has_conflict {
            Tone::Conflict
          } else {
            Tone::Muted
          },
        })
      },
      changes: (review > 0).then_some(Pill {
        count: review as u32,
        tone: if conflict_count > 0 {
          Tone::Conflict
        } else if has_uncommitted {
          Tone::Uncommitted
        } else {
          Tone::Muted
        },
      }),
      logs: loaded.logs_enabled,
      busy: if route.retargeting {
        Some("Rebasing...".into())
      } else {
        None
      },
      loc: loaded.commits.as_ref().and_then(|log| {
        let (mut insertions, mut deletions) = log
          .commits
          .iter()
          .filter(|c| !c.on_target_only)
          .fold((0, 0), |(i, d), c| (i + c.insertions, d + c.deletions));
        if let Some(wc) = &log.tentative_working_copy {
          insertions += wc.commit.insertions;
          deletions += wc.commit.deletions;
        }
        (insertions > 0 || deletions > 0).then_some(Loc {
          insertions,
          deletions,
        })
      }),
    }
  });
  (header, tab_row)
}

#[cfg(test)]
mod remote_header_tests {
  use std::{sync::Arc, time::Duration};

  use super::*;
  use crate::gpui_shell::remote_repo::tests::{repository, workspace_json, FakeTransport};

  fn route(workspace_id: Option<i64>) -> Route {
    serde_json::from_value(serde_json::json!({
      "repoPath": "/srv/app",
      "workspaceId": workspace_id,
      "homeBranch": "main",
      "tab": "changes",
      "retargeting": false,
      "creatingPr": false,
      "runningChecks": [],
      "revision": 0,
    }))
    .unwrap()
  }

  fn answered(latency: Duration) -> (Remote, Arc<FakeTransport>) {
    let transport = Arc::new(FakeTransport::default());
    *transport.latency.lock().unwrap() = latency;
    transport.answer(
      "ListWorkspaces",
      serde_json::json!([workspace_json(3, "feat", "feat/x")]),
    );
    transport.answer(
      "WorkspaceDiff",
      serde_json::json!({
        "committed_files": [],
        "hunks_by_file": [],
        "uncommitted_files": [],
        "conflicted_files": [],
        "too_large_to_render": false,
      }),
    );
    (Remote::new(repository(), transport.clone()), transport)
  }

  fn kinds(transport: &FakeTransport) -> Vec<&'static str> {
    let mut kinds: Vec<_> = transport.sent().iter().map(|r| r.kind_name()).collect();
    kinds.sort_unstable();
    kinds
  }

  #[test]
  fn a_workspace_header_reads_everything_side_by_side() {
    let (remote, transport) = answered(Duration::from_millis(40));
    let started = std::time::Instant::now();
    let header = read_remote_header(&remote, &route(Some(3)));
    let took = started.elapsed();
    assert_eq!(header.workspace.map(|w| w.id), Some(3));
    assert_eq!(
      kinds(&transport),
      [
        "GitRemoteInfo",
        "InspectWorkspace",
        "ListCommits",
        "ListWorkspaces",
        "WorkspaceDiff"
      ]
    );
    // Five serial reads would take 200 ms.
    assert!(took < Duration::from_millis(120), "{took:?}");
  }

  #[test]
  fn a_workspace_on_the_default_branch_counts_changed_files_instead() {
    let (remote, transport) = answered(Duration::ZERO);
    transport.answer(
      "ListWorkspaces",
      serde_json::json!([workspace_json(3, "main", "main")]),
    );
    transport.answer("ListChanges", serde_json::json!([]));
    read_remote_header(&remote, &route(Some(3)));
    assert!(kinds(&transport).contains(&"ListChanges"));
  }

  #[test]
  fn the_home_header_reads_the_changed_files_with_the_rest() {
    let (remote, transport) = answered(Duration::ZERO);
    transport.answer("ListChanges", serde_json::json!([]));
    read_remote_header(&remote, &route(None));
    assert_eq!(
      kinds(&transport),
      ["GitRemoteInfo", "ListChanges", "RepositoryStatus"]
    );
  }
}
