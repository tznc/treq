//! Several workspaces from lines of text, as the New Workspaces dialog and
//! the remote CLI take them: a branch or a title per line, an indented line
//! stacked on the line above. [`plan`] says what each line would do;
//! [`run`] does it, in line order, so a stacked line's base exists first.

use crate::result_ext::ResultExt;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use super::{workspaces, BranchWorkspaceRequest, WorkspaceMetadata};
use crate::{jj, local_db};

/// Why a link line can't be opened here.
const LINK_LINE: &str = "Paste the branch name; links open from the desktop app";

/// A title's branch until the repository sets its own pattern.
pub const DEFAULT_BRANCH_PATTERN: &str = "treq/{name}";

#[allow(clippy::expect_used, reason = "constant pattern")]
static WHITESPACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").expect("regex"));
#[allow(clippy::expect_used, reason = "constant pattern")]
static DASH_RUN: LazyLock<Regex> = LazyLock::new(|| Regex::new("-+").expect("regex"));
#[allow(clippy::expect_used, reason = "constant pattern")]
static TRAILING_DASH_LETTER: LazyLock<Regex> =
  LazyLock::new(|| Regex::new("-[a-z]$").expect("regex"));
#[allow(clippy::expect_used, reason = "constant pattern")]
static TRAILING_SEPARATOR_LETTER: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"[/.\-][a-z]$").expect("regex"));
#[allow(clippy::expect_used, reason = "constant pattern")]
static TRAILING_SEPARATORS: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"[/.\-]+$").expect("regex"));

/// `sanitizeForBranchName`.
#[allow(clippy::expect_used, reason = "constant pattern")]
fn sanitize_for_branch_name(text: &str) -> String {
  let lower: String = text
    .to_lowercase()
    .chars()
    .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c.is_whitespace() || *c == '-')
    .collect();
  let dashed = WHITESPACE.replace_all(&lower, "-");
  let single = DASH_RUN.replace_all(&dashed, "-");
  let cut: String = single.chars().take(50).collect();
  let trimmed = cut.trim_matches('-');
  let sanitized = TRAILING_DASH_LETTER.replace(trimmed, "").to_string();
  if sanitized.is_empty() {
    "unnamed".into()
  } else {
    sanitized
  }
}

/// `cleanBranchNameTrailing`.
#[allow(clippy::expect_used, reason = "constant pattern")]
fn clean_branch_name_trailing(name: &str) -> String {
  let single = &*TRAILING_SEPARATOR_LETTER;
  let separators = &*TRAILING_SEPARATORS;
  let mut result = name.to_string();
  loop {
    let next = separators
      .replace(&single.replace(&result, ""), "")
      .to_string();
    if next == result {
      return result;
    }
    result = next;
  }
}

/// `applyBranchNamePattern`.
pub fn apply_branch_name_pattern(pattern: &str, name: &str) -> String {
  let result =
    clean_branch_name_trailing(&pattern.replace("{name}", &sanitize_for_branch_name(name)));
  if result.is_empty() {
    "unnamed".into()
  } else {
    result
  }
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
  /// A new workspace's title; its branch follows the repository's pattern.
  Title(String),
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

/// Each indented line's parent, by trimmed line: the line above it, so an
/// indented run stacks in order on the line it follows.
pub fn stack_parents(text: &str) -> HashMap<String, String> {
  let mut parents = HashMap::new();
  let mut seen = HashSet::new();
  let mut above: Option<&str> = None;
  for line in text.lines().filter(|l| !l.trim().is_empty()) {
    let trimmed = line.trim();
    // A repeat is the first line again, so it keeps that line's parent.
    if seen.insert(trimmed) {
      if let Some(above) = above.filter(|_| line.starts_with(char::is_whitespace)) {
        parents.insert(trimmed.to_string(), above.to_string());
      }
    }
    above = Some(trimmed);
  }
  parents
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
  if line.contains("://") {
    return Line::Invalid("Not a GitHub link".into());
  }
  if line.chars().any(char::is_whitespace) {
    return Line::Title(line.to_string());
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

/// What a line does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NewWorkspaceStep {
  /// Makes the branch and its workspace.
  Create,
  /// Opens a workspace on a branch that exists.
  Open,
  /// The branch's workspace is open already.
  AlreadyOpen,
}

/// One line of the text, planned (and after [`run`], done).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewWorkspaceRow {
  pub line: String,
  /// How many lines it stacks under.
  pub depth: usize,
  pub branch: Option<String>,
  pub title: Option<String>,
  /// The branch it stacks on.
  pub base: Option<String>,
  /// `None` when the line can't be opened; `error` says why.
  pub step: Option<NewWorkspaceStep>,
  pub error: Option<String>,
  /// The workspace the line opened or made, after a run.
  pub workspace_id: Option<i64>,
}

/// A line's branch, as checking it against the repository found it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Checked {
  branch: String,
  title: Option<String>,
  step: NewWorkspaceStep,
}

/// The plan's rows, from a check of each line. Pure, so it is tested without
/// a repository.
fn plan_rows(
  text: &str,
  stack_on: &str,
  check: impl Fn(&Line) -> Result<Checked, String>,
) -> Vec<NewWorkspaceRow> {
  let parents = stack_parents(text);
  let mut rows: Vec<NewWorkspaceRow> = Vec::new();
  let mut index: HashMap<String, usize> = HashMap::new();
  for (line, parsed) in parse_lines(text) {
    let parent = parents
      .get(&line)
      .and_then(|p| index.get(p))
      .map(|&i| rows[i].clone());
    let checked = check(&parsed)
      .map(|checked| unique_title_branch(checked, &rows, &check))
      .and_then(|checked| match &parent {
        Some(parent) if parent.step.is_none() => {
          Err("Stacks on a line that can't be opened".to_string())
        }
        _ => Ok(checked),
      });
    let base = parent
      .as_ref()
      .and_then(|p| p.branch.clone())
      .unwrap_or_else(|| stack_on.to_string());
    let depth = parent.as_ref().map_or(0, |p| p.depth + 1);
    let row = match checked {
      Ok(checked) => NewWorkspaceRow {
        line: line.clone(),
        depth,
        branch: Some(checked.branch),
        title: checked.title,
        base: Some(base),
        step: Some(checked.step),
        error: None,
        workspace_id: None,
      },
      Err(error) => NewWorkspaceRow {
        line: line.clone(),
        depth,
        branch: None,
        title: None,
        base: None,
        step: None,
        error: Some(error),
        workspace_id: None,
      },
    };
    index.insert(line, rows.len());
    rows.push(row);
  }
  rows
}

/// A new branch named after a title, made unique among the rows above:
/// titles that differ only past the branch name's cut ("Fix the page A",
/// "Fix the page B") would otherwise plan one branch twice, and only the
/// first could be made.
fn unique_title_branch(
  checked: Checked,
  rows: &[NewWorkspaceRow],
  check: &impl Fn(&Line) -> Result<Checked, String>,
) -> Checked {
  let taken = |branch: &str| rows.iter().any(|r| r.branch.as_deref() == Some(branch));
  if checked.title.is_none() || checked.step != NewWorkspaceStep::Create || !taken(&checked.branch)
  {
    return checked;
  }
  (2..100)
    .map(|n| format!("{}-{n}", checked.branch))
    .filter(|branch| !taken(branch))
    .find_map(|branch| {
      check(&Line::Branch(branch))
        .ok()
        .filter(|c| c.step == NewWorkspaceStep::Create)
    })
    .map(|c| Checked {
      title: checked.title.clone(),
      ..c
    })
    .unwrap_or(checked)
}

/// The repository's branch name pattern, from the app settings on this
/// machine, if it has them.
fn branch_pattern(repo_path: &str) -> String {
  let db_path = super::resolve_app_db_path(repo_path);
  db_path
    .exists()
    .then(|| crate::db::Database::new(db_path).ok())
    .flatten()
    .and_then(|db| {
      db.get_repo_setting(repo_path, "branch_name_pattern")
        .ok()
        .flatten()
    })
    .filter(|p| !p.trim().is_empty())
    .unwrap_or_else(|| DEFAULT_BRANCH_PATTERN.to_string())
}

/// What each line of `text` would do in `repo_path`. New workspaces stack
/// on `stack_on`, or the default branch.
pub fn plan(
  repo_path: &str,
  text: &str,
  stack_on: Option<&str>,
) -> Result<Vec<NewWorkspaceRow>, String> {
  let default = super::get_repo_default_branch(repo_path)?;
  let stack_on = stack_on
    .map(str::trim)
    .filter(|b| !b.is_empty())
    .unwrap_or(&default)
    .to_string();
  let pattern = branch_pattern(repo_path);
  let open: HashSet<String> = workspaces::list_workspaces(repo_path)?
    .into_iter()
    .map(|w| w.branch_name)
    .collect();
  let exists = |branch: &str| -> Result<bool, String> {
    let status = jj::check_branch_exists(repo_path, branch).map_err(|e| e.to_string())?;
    Ok(status.local_exists || status.remote_exists)
  };
  let target = |branch: &str, title: Option<&str>| -> Result<Checked, String> {
    if branch == default {
      return Err(format!("'{branch}' is the default branch"));
    }
    let step = if open.contains(branch) {
      NewWorkspaceStep::AlreadyOpen
    } else if exists(branch)? {
      NewWorkspaceStep::Open
    } else {
      workspaces::validate_branch_name(branch).map_err(|err| {
        let prefix = format!("Invalid branch name '{branch}': ");
        format!(
          "Not a valid branch name: {}",
          err.strip_prefix(&prefix).unwrap_or(&err)
        )
      })?;
      NewWorkspaceStep::Create
    };
    Ok(Checked {
      branch: branch.to_string(),
      title: title.map(str::to_string),
      step,
    })
  };
  Ok(plan_rows(text, &stack_on, |line| match line {
    Line::Branch(branch) => {
      // `git branch -r` prints `origin/feat`.
      let short = branch
        .strip_prefix("origin/")
        .filter(|s| exists(s).unwrap_or(false));
      target(short.unwrap_or(branch), None)
    }
    Line::Title(title) => target(&apply_branch_name_pattern(&pattern, title), Some(title)),
    Line::RepoBranch { .. } | Line::Pr { .. } => Err(LINK_LINE.to_string()),
    Line::Invalid(reason) => Err(reason.clone()),
  }))
}

/// [`plan`] without a repository: every branch line is a new branch and a
/// title takes the default pattern. For hosts whose treq can't plan.
pub fn plan_unchecked(text: &str, stack_on: &str) -> Vec<NewWorkspaceRow> {
  plan_rows(text, stack_on, |line| {
    let (branch, title) = match line {
      Line::Branch(branch) => (branch.clone(), None),
      Line::Title(title) => (
        apply_branch_name_pattern(DEFAULT_BRANCH_PATTERN, title),
        Some(title.clone()),
      ),
      Line::RepoBranch { .. } | Line::Pr { .. } => return Err(LINK_LINE.to_string()),
      Line::Invalid(reason) => return Err(reason.clone()),
    };
    Ok(Checked {
      branch,
      title,
      step: NewWorkspaceStep::Create,
    })
  })
}

/// Does what [`plan`] says, line by line: each row gets its workspace, or the
/// error that stopped it. A row stacked on a failed one fails too.
pub fn run(
  repo_path: &str,
  text: &str,
  stack_on: Option<&str>,
) -> Result<Vec<NewWorkspaceRow>, String> {
  workspaces::sync_workspaces(repo_path)?;
  // Remote branches named in the text must be visible before they open.
  jj::jj_git_fetch_unless_recent(repo_path, workspaces::RECENT_FETCH)
    .warn_on_err("jj git fetch unless recent");
  let mut rows = plan(repo_path, text, stack_on)?;
  let default = super::get_repo_default_branch(repo_path)?;
  let copy_files = included_copy_files(repo_path);
  let mut failed: HashSet<String> = HashSet::new();
  for row in &mut rows {
    let (Some(step), Some(branch), Some(base)) = (row.step, row.branch.clone(), row.base.clone())
    else {
      continue;
    };
    if failed.contains(&base) {
      row.error = Some(format!("Not made: {base} failed"));
      failed.insert(branch);
      continue;
    }
    let outcome = match step {
      NewWorkspaceStep::AlreadyOpen => local_db::get_workspace_by_branch(repo_path, &branch)
        .and_then(|w| w.ok_or_else(|| format!("No workspace for {branch}")))
        .map(|w| w.id),
      NewWorkspaceStep::Open => workspaces::open_or_create_synced(
        repo_path,
        &BranchWorkspaceRequest {
          head_branch: branch.clone(),
          base_branch: base.clone(),
          title: row.title.clone(),
          description: None,
        },
      )
      .map(|(w, _)| w.id),
      NewWorkspaceStep::Create => create(
        repo_path,
        &branch,
        &base,
        &default,
        row.title.clone(),
        copy_files.clone(),
      ),
    };
    match outcome {
      Ok(id) => row.workspace_id = Some(id),
      Err(error) => {
        row.error = Some(error);
        failed.insert(branch);
      }
    }
  }
  Ok(rows)
}

/// Makes `branch` from `base` (the default branch's workspaces start there
/// already) and targets `base`.
fn create(
  repo_path: &str,
  branch: &str,
  base: &str,
  default: &str,
  title: Option<String>,
  copy_files: Option<Vec<String>>,
) -> Result<i64, String> {
  let stacked = base != default;
  let metadata = WorkspaceMetadata {
    title,
    ..Default::default()
  };
  let workspace = workspaces::create_new_workspace(
    repo_path,
    branch,
    stacked.then_some(base),
    metadata,
    copy_files,
  )?;
  // Stacked on a workspace's branch, creation already targets it (and starts
  // on its tip); only a plain branch as the base needs the retarget.
  if stacked && workspace.target_branch.as_deref() != Some(base) {
    workspaces::apply_workspace_target_branch_after(
      repo_path,
      workspace.id,
      base,
      workspaces::RECENT_FETCH,
    )?;
  }
  Ok(workspace.id)
}

fn included_copy_files(repo_path: &str) -> Option<Vec<String>> {
  let db_path = super::resolve_app_db_path(repo_path);
  let db = db_path
    .exists()
    .then(|| crate::db::Database::new(db_path).ok())
    .flatten()?;
  let raw = db
    .get_repo_setting(repo_path, "included_copy_files")
    .ok()
    .flatten();
  workspaces::parse_included_copy_files(raw.as_deref())
}

#[cfg(test)]
mod tests {
  use super::*;

  /// `main` exists and `feat/open` has a workspace; anything else is new.
  fn fake(line: &Line) -> Result<Checked, String> {
    let (branch, title) = match line {
      Line::Branch(b) => (b.clone(), None),
      Line::Title(t) => (
        apply_branch_name_pattern(DEFAULT_BRANCH_PATTERN, t),
        Some(t.clone()),
      ),
      Line::Invalid(reason) => return Err(reason.clone()),
      _ => return Err("link".into()),
    };
    if branch == "main" {
      return Err("'main' is the default branch".into());
    }
    let step = if branch == "feat/open" {
      NewWorkspaceStep::AlreadyOpen
    } else {
      NewWorkspaceStep::Create
    };
    Ok(Checked {
      branch,
      title,
      step,
    })
  }

  fn summary(rows: &[NewWorkspaceRow]) -> Vec<(usize, Option<&str>, Option<&str>)> {
    rows
      .iter()
      .map(|r| (r.depth, r.branch.as_deref(), r.base.as_deref()))
      .collect()
  }

  #[test]
  fn an_indented_run_stacks_in_order() {
    let text = "feat/api\n  Add the page\n  feat/tests\nfeat/open\n";
    let rows = plan_rows(text, "main", fake);
    assert_eq!(
      summary(&rows),
      vec![
        (0, Some("feat/api"), Some("main")),
        (1, Some("treq/add-the-page"), Some("feat/api")),
        (2, Some("feat/tests"), Some("treq/add-the-page")),
        (0, Some("feat/open"), Some("main")),
      ]
    );
    assert_eq!(rows[1].title.as_deref(), Some("Add the page"));
    assert_eq!(rows[3].step, Some(NewWorkspaceStep::AlreadyOpen));
  }

  #[test]
  fn a_line_stacked_on_a_bad_line_cannot_open() {
    let rows = plan_rows("main\n  feat/child\n", "main", fake);
    assert_eq!(
      rows[0].error.as_deref(),
      Some("'main' is the default branch")
    );
    assert_eq!(rows[1].step, None);
    assert_eq!(
      rows[1].error.as_deref(),
      Some("Stacks on a line that can't be opened")
    );
  }

  #[test]
  fn titles_that_share_a_branch_get_numbered_branches() {
    // The branch name drops a trailing single letter, so both titles
    // would be treq/fix-the-page.
    let rows = plan_rows(
      "Fix the page A\nFix the page B\n  Add tests\n",
      "main",
      fake,
    );
    assert_eq!(
      summary(&rows),
      vec![
        (0, Some("treq/fix-the-page"), Some("main")),
        (0, Some("treq/fix-the-page-2"), Some("main")),
        (1, Some("treq/add-tests"), Some("treq/fix-the-page-2")),
      ]
    );
    assert_eq!(rows[1].title.as_deref(), Some("Fix the page B"));
    assert_eq!(rows[1].step, Some(NewWorkspaceStep::Create));
  }

  #[test]
  fn new_workspaces_stack_on_the_chosen_branch() {
    let rows = plan_rows("feat/x", "release", fake);
    assert_eq!(summary(&rows), vec![(0, Some("feat/x"), Some("release"))]);
  }
}
