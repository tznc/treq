//! What the sidebar's Open Workspaces dialog reads and does: which branch
//! each pasted line names, whether it can be opened, and the backend calls
//! that open them.

use std::collections::HashSet;

use serde::Deserialize;

use super::{github_data::GithubHost, workspace_dialog_data::WorkspaceDialogHost};
use crate::{
  core::{self, BranchWorkspaceRequest},
  github::GhPullRequest,
  gpui_shell::native::Backend,
};

/// How many pull requests the dialog lists.
const PR_LIMIT: u32 = 50;

/// The dialog's route: the repository on screen and every repository the
/// sidebar shows, whose GitHub links the dialog accepts.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Route {
  pub repo_path: String,
  pub repos: Vec<String>,
}

/// What a pasted line names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
  /// A branch of the repository on screen.
  Branch(String),
  /// `github.com/{repo}/tree/{branch}`.
  RepoBranch { repo: String, branch: String },
  /// `github.com/{repo}/pull/{number}`.
  Pr { repo: String, number: u64 },
  /// Not a branch or a link the dialog reads, and why.
  Invalid(String),
}

/// The pasted text's lines, trimmed, without blanks or repeats.
pub fn parse_lines(text: &str) -> Vec<(String, Line)> {
  let mut seen = HashSet::new();
  text
    .lines()
    .map(str::trim)
    .filter(|line| !line.is_empty() && seen.insert(line.to_string()))
    .map(|line| (line.to_string(), parse_line(line)))
    .collect()
}

pub fn parse_line(line: &str) -> Line {
  let line = line.trim();
  let bare = line
    .strip_prefix("https://")
    .or_else(|| line.strip_prefix("http://"))
    .unwrap_or(line);
  let bare = bare.strip_prefix("www.").unwrap_or(bare);
  if let Some(path) = bare.strip_prefix("github.com/") {
    return parse_github_path(path);
  }
  if line.contains("://") || line.chars().any(char::is_whitespace) {
    return Line::Invalid("Not a branch name or a GitHub link".into());
  }
  let branch = line.strip_prefix("refs/heads/").unwrap_or(line);
  let branch = branch.strip_suffix("@origin").unwrap_or(branch);
  Line::Branch(branch.to_string())
}

fn parse_github_path(path: &str) -> Line {
  let path = path.split(['?', '#']).next().unwrap_or_default();
  let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
  let [owner, name, rest @ ..] = parts.as_slice() else {
    return Line::Invalid("Not a GitHub repository link".into());
  };
  let repo = format!("{owner}/{}", name.trim_end_matches(".git"));
  match rest {
    ["pull", number, ..] => match number.parse() {
      Ok(number) => Line::Pr { repo, number },
      Err(_) => Line::Invalid(format!("'{number}' is not a pull request number")),
    },
    ["tree", branch @ ..] if !branch.is_empty() => Line::RepoBranch {
      repo,
      branch: branch.join("/"),
    },
    _ => Line::Invalid("Paste a pull request or branch link".into()),
  }
}

/// A workspace the dialog will open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
  pub repo_path: String,
  pub branch: String,
  /// The rebase target: the pull request's base, or the default branch.
  pub base: String,
  pub title: Option<String>,
  pub pr: Option<u64>,
  /// A workspace for the branch is open already.
  pub existing: bool,
}

impl Target {
  pub fn request(&self) -> BranchWorkspaceRequest {
    BranchWorkspaceRequest {
      head_branch: self.branch.clone(),
      base_branch: self.base.clone(),
      title: self.title.clone(),
      description: self.pr.map(|n| format!("From GitHub PR #{n}")),
    }
  }

  /// A pull request of the repository on screen.
  pub fn from_pr(repo_path: &str, pr: &GhPullRequest, existing: bool) -> Self {
    Self {
      repo_path: repo_path.to_string(),
      branch: pr.head_ref_name.clone(),
      base: pr.base_ref_name.clone(),
      title: Some(pr.title.clone()),
      pr: Some(pr.number),
      existing,
    }
  }
}

/// What checking a line found.
pub type Checked = Result<Target, String>;

/// What the dialog asks of the backend; faked in tests. Every call blocks.
pub trait OpenWorkspacesHost: GithubHost + WorkspaceDialogHost {
  /// Fetches the repository's remote, so its branches can be checked.
  fn fetch(&self, repo_path: &str);
  /// Open pull requests, newest first: the user's own, or everyone's
  /// matching `query`.
  fn open_prs(&self, repo: &str, mine: bool, query: &str) -> Result<Vec<GhPullRequest>, String>;
  /// Opens or creates a workspace for each request: its id, and whether it
  /// was made.
  fn open_workspaces(
    &self,
    repo_path: &str,
    requests: &[BranchWorkspaceRequest],
  ) -> Vec<Result<(i64, bool), String>>;
}

impl OpenWorkspacesHost for Backend {
  fn fetch(&self, repo_path: &str) {
    if let Err(err) = crate::jj::jj_git_fetch(repo_path) {
      log::warn!("open workspaces: fetch {repo_path}: {err}");
    }
  }

  fn open_prs(&self, repo: &str, mine: bool, query: &str) -> Result<Vec<GhPullRequest>, String> {
    let gh = crate::binary_paths::get_binary_path("gh")
      .or_else(|| crate::binary_paths::detect_binary("gh"))
      .ok_or_else(|| "gh CLI not found".to_string())?;
    let path = crate::binary_paths::get_extended_path();
    crate::github::gh_search_open_prs_impl(&gh, repo, mine, query, PR_LIMIT, &path)
  }

  fn open_workspaces(
    &self,
    repo_path: &str,
    requests: &[BranchWorkspaceRequest],
  ) -> Vec<Result<(i64, bool), String>> {
    core::open_or_create_workspaces(repo_path, requests)
      .into_iter()
      .map(|result| {
        let (workspace, created) = result?;
        if created {
          crate::commands::workspace::spawn_setup_script_if_configured(&workspace);
        }
        Ok((workspace.id, created))
      })
      .collect()
  }
}

/// The repositories' `owner/name`, by path; one without a GitHub remote is
/// left out.
pub fn remotes(host: &dyn OpenWorkspacesHost, repos: &[String]) -> Vec<(String, String)> {
  repos
    .iter()
    .filter_map(|path| {
      let remote = host.remote(path).ok().flatten()?;
      Some((path.clone(), remote.full_name))
    })
    .collect()
}

/// Checks `line` against the backend: the repository it belongs to, that
/// its branch exists, and whether a workspace is open for it.
pub fn check(
  host: &dyn OpenWorkspacesHost,
  repo_path: &str,
  remotes: &[(String, String)],
  line: &Line,
) -> Checked {
  let repo_of = |full_name: &str| {
    remotes
      .iter()
      .find(|(_, name)| name.eq_ignore_ascii_case(full_name))
      .map(|(path, _)| path.clone())
      .ok_or_else(|| format!("{full_name} is not one of your repositories"))
  };
  match line {
    Line::Invalid(reason) => Err(reason.clone()),
    Line::Branch(branch) => {
      // `git branch -r` prints `origin/feat`.
      let short = branch.strip_prefix("origin/");
      let branch = if branch_exists(host, repo_path, branch)? {
        branch.as_str()
      } else if let Some(short) =
        short.filter(|s| branch_exists(host, repo_path, s).unwrap_or(false))
      {
        short
      } else {
        return Err(format!("No branch '{branch}' locally or on the remote"));
      };
      branch_target(host, repo_path, branch)
    }
    Line::RepoBranch { repo, branch } => {
      let path = repo_of(repo)?;
      if !branch_exists(host, &path, branch)? {
        return Err(format!("No branch '{branch}' in {repo}"));
      }
      branch_target(host, &path, branch)
    }
    Line::Pr { repo, number } => {
      let path = repo_of(repo)?;
      let pr = host.pr(repo, *number)?;
      if !pr.state.eq_ignore_ascii_case("open") {
        return Err(format!(
          "Pull request #{number} is {}",
          pr.state.to_lowercase()
        ));
      }
      if !branch_exists(host, &path, &pr.head_ref_name)? {
        return Err(format!(
          "Pull request #{number}'s branch '{}' is not on the remote",
          pr.head_ref_name
        ));
      }
      let existing = is_open(host, &path, &pr.head_ref_name);
      Ok(Target::from_pr(&path, &pr, existing))
    }
  }
}

fn branch_exists(
  host: &dyn OpenWorkspacesHost,
  repo_path: &str,
  branch: &str,
) -> Result<bool, String> {
  let status = host.branch_status(repo_path, branch)?;
  Ok(status.local_exists || status.remote_exists)
}

fn branch_target(host: &dyn OpenWorkspacesHost, repo_path: &str, branch: &str) -> Checked {
  let base = WorkspaceDialogHost::default_branch(host, repo_path)?;
  if branch == base {
    return Err(format!("'{branch}' is the default branch"));
  }
  Ok(Target {
    repo_path: repo_path.to_string(),
    branch: branch.to_string(),
    base,
    title: None,
    pr: None,
    existing: is_open(host, repo_path, branch),
  })
}

/// Whether a workspace is open for `branch`.
pub fn is_open(host: &dyn OpenWorkspacesHost, repo_path: &str, branch: &str) -> bool {
  GithubHost::workspaces(host, repo_path)
    .map(|all| all.iter().any(|w| w.branch_name == branch))
    .unwrap_or(false)
}

#[cfg(test)]
mod tests {
  use super::*;

  fn branch(name: &str) -> Line {
    Line::Branch(name.into())
  }

  #[test]
  fn parse_line_reads_branch_names() {
    assert_eq!(parse_line("feat/login"), branch("feat/login"));
    assert_eq!(parse_line(" feat/login@origin "), branch("feat/login"));
    assert_eq!(parse_line("refs/heads/fix"), branch("fix"));
  }

  #[test]
  fn parse_line_reads_pull_request_links() {
    let pr = Line::Pr {
      repo: "acme/app".into(),
      number: 42,
    };
    assert_eq!(parse_line("https://github.com/acme/app/pull/42"), pr);
    assert_eq!(parse_line("github.com/acme/app/pull/42/files#diff"), pr);
    assert_eq!(
      parse_line("https://www.github.com/acme/app.git/pull/42"),
      pr
    );
  }

  #[test]
  fn parse_line_reads_branch_links() {
    assert_eq!(
      parse_line("https://github.com/acme/app/tree/feat/login?tab=readme"),
      Line::RepoBranch {
        repo: "acme/app".into(),
        branch: "feat/login".into()
      }
    );
  }

  #[test]
  fn parse_line_rejects_other_text() {
    for line in [
      "https://github.com/acme/app",
      "https://github.com/acme/app/issues/3",
      "https://github.com/acme/app/pull/abc",
      "https://gitlab.com/acme/app/pull/1",
      "two words",
    ] {
      assert!(matches!(parse_line(line), Line::Invalid(_)), "{line}");
    }
  }

  #[test]
  fn parse_lines_skips_blanks_and_repeats() {
    let lines = parse_lines("feat/a\n\n  feat/b \nfeat/a\n");
    let names: Vec<&str> = lines.iter().map(|(raw, _)| raw.as_str()).collect();
    assert_eq!(names, ["feat/a", "feat/b"]);
  }
}
