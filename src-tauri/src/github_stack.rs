//! GitHub's native stacked pull requests: which stack a pull request is in.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::github::{
  check_gh_output, check_graphql_errors, spawn_with_etxtbsy_retry, GhMergeMethod,
};
use crate::lock_ext::LockExt as _;

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

/// The `gh stack` extension's repository.
const EXTENSION: &str = "github/gh-stack";

/// Runs `gh args` in `dir` (when given); its output.
fn run(
  gh_path: &str,
  dir: Option<&str>,
  args: &[&str],
  extended_path: &str,
) -> Result<Vec<u8>, String> {
  let mut cmd = std::process::Command::new(gh_path);
  cmd.args(args).env("PATH", extended_path);
  if let Some(dir) = dir {
    cmd.current_dir(dir);
  }
  check_gh_output(spawn_with_etxtbsy_retry(cmd)?)
}

/// Whether the `gh stack` extension is installed.
pub fn gh_stack_installed_impl(gh_path: &str, extended_path: &str) -> Result<bool, String> {
  let out = run(gh_path, None, &["extension", "list"], extended_path)?;
  Ok(
    String::from_utf8_lossy(&out)
      .lines()
      .any(|line| line.split_whitespace().any(|field| field == EXTENSION)),
  )
}

/// Installs the `gh stack` extension.
pub fn gh_stack_install_impl(gh_path: &str, extended_path: &str) -> Result<(), String> {
  run(
    gh_path,
    None,
    &["extension", "install", EXTENSION],
    extended_path,
  )
  .map(drop)
}

/// Puts pull requests `prs`, bottom first, in one stack on GitHub. Adds to
/// a stack they are already in; never takes one out.
pub fn gh_stack_link_impl(
  gh_path: &str,
  repo_path: &str,
  prs: &[u64],
  extended_path: &str,
) -> Result<(), String> {
  let numbers: Vec<String> = prs.iter().map(u64::to_string).collect();
  let mut args = vec!["stack", "link"];
  args.extend(numbers.iter().map(String::as_str));
  run(gh_path, Some(repo_path), &args, extended_path).map(drop)
}

/// Takes the open pull requests of stack `number` out of it on GitHub.
pub fn gh_stack_unstack_impl(
  gh_path: &str,
  repo_path: &str,
  number: u64,
  extended_path: &str,
) -> Result<(), String> {
  let number = number.to_string();
  run(
    gh_path,
    Some(repo_path),
    &["stack", "unstack", &number],
    extended_path,
  )
  .map(drop)
}

/// Merges all of stack `number` at once, without asking.
pub fn gh_stack_merge_impl(
  gh_path: &str,
  repo_path: &str,
  number: u64,
  method: GhMergeMethod,
  extended_path: &str,
) -> Result<(), String> {
  let number = number.to_string();
  run(
    gh_path,
    Some(repo_path),
    &["stack", "merge", &number, method.flag(), "--yes"],
    extended_path,
  )
  .map(drop)
}

type Kept = (Instant, Option<GhStack>);

/// Each pull request's stack, kept for a while: the stack card asks again
/// on every revalidation, and each ask is a network call.
pub struct StackCache {
  ttl: Duration,
  /// By `(repo_path, number)`: when it was read, and what it found.
  entries: Mutex<HashMap<(String, u64), Kept>>,
}

impl StackCache {
  pub fn new(ttl: Duration) -> Self {
    Self {
      ttl,
      entries: Mutex::default(),
    }
  }

  /// Forgets every stack, as after changing one on GitHub.
  pub fn clear(&self) {
    self.entries.lock_or_recover().clear();
  }

  /// The stack kept for pull request `number` in `repo_path` as of `now`,
  /// else what `read` finds, kept from `now`.
  pub fn get_or(
    &self,
    repo_path: &str,
    number: u64,
    now: Instant,
    read: impl FnOnce() -> Option<GhStack>,
  ) -> Option<GhStack> {
    let key = (repo_path.to_string(), number);
    let kept = self.entries.lock_or_recover().get(&key).cloned();
    if let Some((_, stack)) = kept.filter(|(at, _)| now.duration_since(*at) < self.ttl) {
      return stack;
    }
    let stack = read();
    self
      .entries
      .lock_or_recover()
      .insert(key, (now, stack.clone()));
    stack
  }
}

#[cfg(test)]
mod cache_tests {
  use super::*;

  fn stack(number: u64) -> Option<GhStack> {
    Some(GhStack {
      number,
      base_ref_name: "main".into(),
      entries: vec![],
    })
  }

  #[test]
  fn keeps_a_read_until_it_expires() {
    let cache = StackCache::new(Duration::from_secs(60));
    let start = Instant::now();
    cache.get_or("/repo", 42, start, || stack(12));

    let kept = cache.get_or("/repo", 42, start + Duration::from_secs(59), || stack(13));
    let fresh = cache.get_or("/repo", 42, start + Duration::from_secs(61), || stack(14));

    assert_eq!(kept, stack(12));
    assert_eq!(fresh, stack(14));
  }

  #[test]
  fn clearing_reads_every_stack_again() {
    let cache = StackCache::new(Duration::from_secs(60));
    let now = Instant::now();
    cache.get_or("/repo", 42, now, || stack(12));

    cache.clear();

    assert_eq!(cache.get_or("/repo", 42, now, || None), None);
  }

  #[test]
  fn keeps_each_pull_request_apart() {
    let cache = StackCache::new(Duration::from_secs(60));
    let now = Instant::now();
    cache.get_or("/repo", 42, now, || stack(12));

    assert_eq!(cache.get_or("/repo", 43, now, || None), None);
    assert_eq!(cache.get_or("/other", 42, now, || stack(5)), stack(5));
  }
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

#[cfg(test)]
#[cfg(unix)]
mod cli_tests {
  use super::*;
  use std::fs;
  use tempfile::TempDir;

  /// A fake `gh` running `body` after recording its arguments
  /// (`\0`-separated) and working directory.
  fn fake_gh(dir: &TempDir, body: &str) -> String {
    use std::os::unix::fs::PermissionsExt;
    let gh = dir.path().join("gh");
    fs::write(
      &gh,
      format!(
        "#!/bin/sh\nfor a in \"$@\"; do printf '%s\\0' \"$a\"; done > '{args}'\npwd > '{cwd}'\n{body}\n",
        args = dir.path().join("args").display(),
        cwd = dir.path().join("cwd").display(),
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

  fn ran_in(dir: &TempDir, repo: &TempDir) -> bool {
    let cwd = fs::read_to_string(dir.path().join("cwd")).unwrap();
    fs::canonicalize(cwd.trim()).unwrap() == fs::canonicalize(repo.path()).unwrap()
  }

  const PATH: &str = "/usr/bin:/bin";

  #[test]
  fn the_extension_is_installed_when_gh_lists_it() {
    let bin = TempDir::new().unwrap();
    let gh = fake_gh(
      &bin,
      "printf 'gh dash\\tdlvhdr/gh-dash\\tv4.0.0\\ngh stack\\tgithub/gh-stack\\tv0.4.1\\n'",
    );

    assert_eq!(gh_stack_installed_impl(&gh, PATH), Ok(true));
    assert_eq!(args(&bin), ["extension", "list"]);
  }

  #[test]
  fn the_extension_is_missing_when_gh_does_not_list_it() {
    let bin = TempDir::new().unwrap();
    let gh = fake_gh(&bin, "printf 'gh dash\\tdlvhdr/gh-dash\\tv4.0.0\\n'");

    assert_eq!(gh_stack_installed_impl(&gh, PATH), Ok(false));
  }

  #[test]
  fn installs_the_extension() {
    let bin = TempDir::new().unwrap();
    let gh = fake_gh(&bin, "");

    gh_stack_install_impl(&gh, PATH).unwrap();

    assert_eq!(args(&bin), ["extension", "install", "github/gh-stack"]);
  }

  #[test]
  fn links_pull_requests_bottom_first_in_the_repository() {
    let (bin, repo) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let gh = fake_gh(&bin, "");

    gh_stack_link_impl(&gh, repo.path().to_str().unwrap(), &[40, 41, 42], PATH).unwrap();

    assert_eq!(args(&bin), ["stack", "link", "40", "41", "42"]);
    assert!(ran_in(&bin, &repo));
  }

  #[test]
  fn unstacks_the_stack_on_github() {
    let (bin, repo) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let gh = fake_gh(&bin, "");

    gh_stack_unstack_impl(&gh, repo.path().to_str().unwrap(), 12, PATH).unwrap();

    assert_eq!(args(&bin), ["stack", "unstack", "12"]);
    assert!(ran_in(&bin, &repo));
  }

  #[test]
  fn merges_the_whole_stack_without_prompting() {
    let (bin, repo) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let gh = fake_gh(&bin, "");

    gh_stack_merge_impl(
      &gh,
      repo.path().to_str().unwrap(),
      12,
      GhMergeMethod::Squash,
      PATH,
    )
    .unwrap();

    assert_eq!(args(&bin), ["stack", "merge", "12", "--squash", "--yes"]);
    assert!(ran_in(&bin, &repo));
  }

  #[test]
  fn a_failed_command_reports_what_gh_said() {
    let bin = TempDir::new().unwrap();
    let gh = fake_gh(&bin, "echo 'stack 12 has merge conflicts' >&2; exit 1");

    let err = gh_stack_unstack_impl(&gh, "/", 12, PATH).unwrap_err();

    assert!(err.contains("stack 12 has merge conflicts"), "{err}");
  }
}
