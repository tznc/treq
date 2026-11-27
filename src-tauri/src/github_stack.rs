//! GitHub's native stacked pull requests: which stack a pull request is in.

use serde::Deserialize;

use crate::github::{check_gh_output, check_graphql_errors, spawn_with_etxtbsy_retry};

/// A stack of pull requests on GitHub.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GhStack {
  /// Numbers the stack within its repository.
  pub number: u64,
  /// The branch the bottom pull request targets.
  pub base_ref_name: String,
  /// Bottom first.
  pub entries: Vec<GhStackEntry>,
}

/// One pull request in a [`GhStack`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GhStackEntry {
  pub number: u64,
  pub head_ref_name: String,
  pub state: String,
}

const PR_STACK_QUERY: &str = "query($owner: String!, $name: String!, $number: Int!) { repository(owner: $owner, name: $name) { pullRequest(number: $number) { stack { number baseRefName entries(first: 100) { nodes { position pullRequest { number headRefName state } } } } } } }";

#[derive(Deserialize)]
struct Response {
  data: Option<Data>,
}

#[derive(Deserialize)]
struct Data {
  repository: Option<Repository>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Repository {
  pull_request: Option<PullRequest>,
}

#[derive(Deserialize)]
struct PullRequest {
  stack: Option<StackNode>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StackNode {
  number: u64,
  base_ref_name: String,
  entries: Entries,
}

#[derive(Deserialize)]
struct Entries {
  nodes: Vec<Option<EntryNode>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EntryNode {
  position: u32,
  pull_request: Option<EntryPr>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EntryPr {
  number: u64,
  head_ref_name: String,
  state: String,
}

impl From<StackNode> for GhStack {
  fn from(node: StackNode) -> Self {
    let mut entries: Vec<EntryNode> = node.entries.nodes.into_iter().flatten().collect();
    // Position 1 sits on the base branch.
    entries.sort_by_key(|e| e.position);
    GhStack {
      number: node.number,
      base_ref_name: node.base_ref_name,
      entries: entries
        .into_iter()
        .filter_map(|e| e.pull_request)
        .map(|pr| GhStackEntry {
          number: pr.number,
          head_ref_name: pr.head_ref_name,
          state: pr.state,
        })
        .collect(),
    }
  }
}

/// The stack pull request `pr_number` is in on GitHub; `None` when it is in
/// none. Runs in `repo_path`, so `gh` picks the repository it would for
/// `gh pr view`.
pub fn gh_pr_stack_impl(
  gh_path: &str,
  repo_path: &str,
  pr_number: u64,
  extended_path: &str,
) -> Result<Option<GhStack>, String> {
  let query = format!("query={PR_STACK_QUERY}");
  let number = format!("number={pr_number}");
  let mut cmd = std::process::Command::new(gh_path);
  cmd
    // `{owner}` and `{repo}` are gh placeholders for the repository it finds here.
    .args([
      "api",
      "graphql",
      "-f",
      &query,
      "-F",
      "owner={owner}",
      "-F",
      "name={repo}",
      "-F",
      &number,
    ])
    .current_dir(repo_path)
    .env("PATH", extended_path);
  let bytes = check_gh_output(spawn_with_etxtbsy_retry(cmd)?)?;
  check_graphql_errors(&bytes)?;
  let response: Response =
    serde_json::from_slice(&bytes).map_err(|e| format!("Failed to parse gh output: {e}"))?;
  Ok(
    response
      .data
      .and_then(|d| d.repository)
      .and_then(|r| r.pull_request)
      .and_then(|pr| pr.stack)
      .map(GhStack::from),
  )
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
  use super::*;
  use std::fs;
  use tempfile::TempDir;

  /// A fake `gh` that records its arguments (`\0`-separated) and working
  /// directory, then prints `stdout`.
  fn fake_gh(dir: &TempDir, stdout: &str) -> String {
    use std::os::unix::fs::PermissionsExt;
    let out = dir.path().join("out.json");
    fs::write(&out, stdout).unwrap();
    let gh = dir.path().join("gh");
    fs::write(
      &gh,
      format!(
        "#!/bin/sh\nfor a in \"$@\"; do printf '%s\\0' \"$a\"; done > '{args}'\npwd > '{cwd}'\ncat '{out}'\n",
        args = dir.path().join("args").display(),
        cwd = dir.path().join("cwd").display(),
        out = out.display(),
      ),
    )
    .unwrap();
    fs::set_permissions(&gh, fs::Permissions::from_mode(0o755)).unwrap();
    gh.to_str().unwrap().to_string()
  }

  fn args(dir: &TempDir) -> Vec<String> {
    fs::read_to_string(dir.path().join("args"))
      .unwrap()
      .split('\0')
      .filter(|a| !a.is_empty())
      .map(str::to_string)
      .collect()
  }

  const STACK: &str = r#"{"data":{"repository":{"pullRequest":{"stack":{"number":12,"baseRefName":"main","entries":{"nodes":[
    {"position":2,"pullRequest":{"number":42,"headRefName":"feat/toggle","state":"OPEN"}},
    {"position":1,"pullRequest":{"number":40,"headRefName":"fix/schedule","state":"MERGED"}},
    {"position":3,"pullRequest":null}
  ]}}}}}}"#;

  #[test]
  fn reads_the_stack_bottom_first() {
    let bin = TempDir::new().unwrap();
    let gh = fake_gh(&bin, STACK);

    let stack = gh_pr_stack_impl(&gh, "/", 42, "/usr/bin:/bin")
      .unwrap()
      .expect("stack");

    assert_eq!(stack.number, 12);
    assert_eq!(stack.base_ref_name, "main");
    assert_eq!(
      stack.entries,
      [
        GhStackEntry {
          number: 40,
          head_ref_name: "fix/schedule".into(),
          state: "MERGED".into(),
        },
        GhStackEntry {
          number: 42,
          head_ref_name: "feat/toggle".into(),
          state: "OPEN".into(),
        },
      ]
    );
  }

  #[test]
  fn queries_the_pull_request_in_the_current_repository() {
    let bin = TempDir::new().unwrap();
    let repo = TempDir::new().unwrap();
    let gh = fake_gh(&bin, STACK);

    gh_pr_stack_impl(&gh, repo.path().to_str().unwrap(), 42, "/usr/bin:/bin").unwrap();

    let args = args(&bin);
    assert_eq!(&args[..2], ["api", "graphql"], "{args:?}");
    for field in ["owner={owner}", "name={repo}", "number=42"] {
      assert!(args.contains(&field.to_string()), "{field} in {args:?}");
    }
    let query = args.iter().find(|a| a.starts_with("query=")).unwrap();
    assert!(query.contains("stack {"), "{query}");
    let cwd = fs::read_to_string(bin.path().join("cwd")).unwrap();
    assert_eq!(
      fs::canonicalize(cwd.trim()).unwrap(),
      fs::canonicalize(repo.path()).unwrap()
    );
  }

  #[test]
  fn is_none_when_the_pull_request_is_in_no_stack() {
    let bin = TempDir::new().unwrap();
    let gh = fake_gh(
      &bin,
      r#"{"data":{"repository":{"pullRequest":{"stack":null}}}}"#,
    );

    assert_eq!(gh_pr_stack_impl(&gh, "/", 42, "/usr/bin:/bin"), Ok(None));
  }

  #[test]
  fn reports_graphql_errors() {
    let bin = TempDir::new().unwrap();
    let gh = fake_gh(
      &bin,
      r#"{"errors":[{"message":"Field 'stack' doesn't exist on type 'PullRequest'"}]}"#,
    );

    let err = gh_pr_stack_impl(&gh, "/", 42, "/usr/bin:/bin").unwrap_err();

    assert!(err.contains("Field 'stack' doesn't exist"), "{err}");
  }
}
