use crate::result_ext::ResultExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct GitRemoteInfo {
  pub owner: String,
  pub repo: String,
  pub full_name: String,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PrInfo {
  pub number: u64,
  pub title: String,
  pub state: String,
  pub url: String,
  pub head_ref_name: String,
  pub base_ref_name: String,
  pub merge_state_status: Option<String>,
  #[serde(default)]
  pub is_draft: bool,
  /// The PR description, markdown. Empty when the PR has none.
  #[serde(default)]
  pub body: String,
}

/// Parse owner/repo from a GitHub remote URL in SSH or HTTPS form.
/// Returns None if the URL is not a recognized GitHub remote.
pub fn parse_github_remote(url: &str) -> Option<GitRemoteInfo> {
  let url = url.trim();

  // SSH: git@github.com:owner/repo.git
  if let Some(rest) = url.strip_prefix("git@github.com:") {
    let path = rest.trim_end_matches(".git");
    let (owner, repo) = valid_repo_path(path)?;
    return Some(GitRemoteInfo {
      owner: owner.to_string(),
      repo: repo.to_string(),
      full_name: format!("{owner}/{repo}"),
    });
  }

  // HTTPS: https://github.com/owner/repo[.git]
  for prefix in &["https://github.com/", "http://github.com/"] {
    if let Some(rest) = url.strip_prefix(prefix) {
      let path = rest.trim_end_matches(".git");
      let (owner, repo) = valid_repo_path(path)?;
      return Some(GitRemoteInfo {
        owner: owner.to_string(),
        repo: repo.to_string(),
        full_name: format!("{owner}/{repo}"),
      });
    }
  }

  None
}

fn valid_repo_path(path: &str) -> Option<(&str, &str)> {
  let mut parts = path.split('/');
  let owner = parts.next()?;
  let repo = parts.next()?;
  if owner.is_empty() || repo.is_empty() || parts.next().is_some() {
    return None;
  }
  Some((owner, repo))
}

/// Read the GitHub remote URL from .git/config and parse owner/repo.
/// Returns None if no GitHub remote is found or the directory is not a git repo.
pub fn get_git_remote_url_impl(repo_path: &str) -> Result<Option<GitRemoteInfo>, String> {
  let dot_git = Path::new(repo_path).join(".git");
  let git_dir = if dot_git.is_dir() {
    dot_git
  } else if dot_git.is_file() {
    let pointer = std::fs::read_to_string(&dot_git).map_err(|e| e.to_string())?;
    let path = pointer
      .trim()
      .strip_prefix("gitdir:")
      .map(str::trim)
      .ok_or_else(|| "Malformed .git worktree pointer".to_string())?;
    let path = Path::new(path);
    if path.is_absolute() {
      path.to_path_buf()
    } else {
      Path::new(repo_path).join(path)
    }
  } else {
    return Ok(None);
  };
  let common_dir = git_dir.join("commondir");
  let config_dir = if common_dir.is_file() {
    let common = std::fs::read_to_string(common_dir).map_err(|e| e.to_string())?;
    git_dir.join(common.trim())
  } else if git_dir.join("config").is_file() {
    git_dir
  } else if let Some(common) = git_dir
    .parent()
    .filter(|parent| parent.file_name().is_some_and(|name| name == "worktrees"))
    .and_then(Path::parent)
  {
    common.to_path_buf()
  } else {
    git_dir
  };
  let git_config_path = config_dir.join("config");
  if !git_config_path.exists() {
    return Ok(None);
  }

  let contents = std::fs::read_to_string(&git_config_path).map_err(|e| e.to_string())?;

  // Parse INI-style .git/config: find [remote "origin"] section and its url
  let mut in_origin_remote = false;
  for line in contents.lines() {
    let trimmed = line.trim();
    if trimmed.starts_with('[') {
      in_origin_remote = trimmed == r#"[remote "origin"]"#;
      continue;
    }
    if in_origin_remote {
      if let Some(rest) = trimmed.strip_prefix("url") {
        let rest = rest.trim_start();
        if let Some(url) = rest.strip_prefix('=') {
          if let Some(info) = parse_github_remote(url.trim()) {
            return Ok(Some(info));
          }
        }
      }
    }
  }

  Ok(None)
}

// ── GitHub Issues / PRs ─────────────────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct GhLabel {
  pub name: String,
  pub color: String,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq)]
pub struct GhAuthor {
  pub login: String,
  #[serde(default)]
  pub avatar_url: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
#[serde(rename_all(deserialize = "camelCase"))]
pub struct GhIssueComment {
  pub id: String,
  pub body: String,
  pub author: GhAuthor,
  #[serde(alias = "created_at")]
  pub created_at: String,
  /// `gh` gives the counts; `gh_comment_reactions_impl` adds whether the
  /// viewer reacted.
  #[serde(default, rename(deserialize = "reactionGroups"), alias = "reactions")]
  pub reactions: Vec<GhReactionGroup>,
}

/// One of GitHub's eight reactions on a comment, as `ReactionGroup`.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(from = "RawReactionGroup")]
pub struct GhReactionGroup {
  /// `ReactionContent`: `THUMBS_UP`, `HEART`, ...
  pub content: String,
  pub count: u32,
  pub viewer_has_reacted: bool,
}

/// A `ReactionGroup` as `gh --json` (`users`) or GraphQL (`reactors`)
/// spells it, or as [`GhReactionGroup`] serializes it (`count`).
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawReactionGroup {
  content: String,
  #[serde(default)]
  count: Option<u32>,
  #[serde(default)]
  users: Option<TotalCount>,
  #[serde(default)]
  reactors: Option<TotalCount>,
  #[serde(default, alias = "viewer_has_reacted")]
  viewer_has_reacted: bool,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct TotalCount {
  total_count: u32,
}

impl From<RawReactionGroup> for GhReactionGroup {
  fn from(raw: RawReactionGroup) -> Self {
    GhReactionGroup {
      content: raw.content,
      count: raw
        .count
        .or_else(|| raw.reactors.or(raw.users).map(|c| c.total_count))
        .unwrap_or(0),
      viewer_has_reacted: raw.viewer_has_reacted,
    }
  }
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
#[serde(rename_all(deserialize = "camelCase"))]
pub struct GhIssue {
  pub number: u64,
  pub title: String,
  pub state: String,
  pub url: String,
  pub body: Option<String>,
  pub author: GhAuthor,
  pub labels: Vec<GhLabel>,
  #[serde(alias = "created_at")]
  pub created_at: String,
  #[serde(alias = "updated_at")]
  pub updated_at: String,
  pub comments: Option<Vec<GhIssueComment>>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
#[serde(rename_all(deserialize = "camelCase"))]
pub struct GhPullRequest {
  pub number: u64,
  pub title: String,
  pub state: String,
  pub url: String,
  pub body: Option<String>,
  pub author: GhAuthor,
  pub labels: Vec<GhLabel>,
  #[serde(alias = "head_ref_name")]
  pub head_ref_name: String,
  #[serde(alias = "base_ref_name")]
  pub base_ref_name: String,
  #[serde(alias = "merge_state_status")]
  pub merge_state_status: Option<String>,
  #[serde(alias = "created_at")]
  pub created_at: String,
  #[serde(alias = "updated_at")]
  pub updated_at: String,
  pub comments: Option<Vec<GhIssueComment>>,
  #[serde(default, alias = "is_draft")]
  pub is_draft: bool,
}

/// ETXTBSY: the target binary is momentarily open for writing elsewhere (e.g.
/// another test process still finalizing a freshly-written fake `gh` script
/// under heavy parallelism). The exec never started in this case, so retrying
/// is always safe -- including for mutating `gh` subcommands.
const ETXTBSY: i32 = 26;

/// How long one `gh` call may run. A hung `gh` (network stall, credential
/// helper) would otherwise block its caller, such as the PR status poller,
/// for good.
const GH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

fn spawn_with_etxtbsy_retry(
  mut cmd: std::process::Command,
) -> Result<std::process::Output, String> {
  use std::process::Stdio;

  // gh must never stop to ask a question: there is no one to answer it.
  cmd
    .env("GH_PROMPT_DISABLED", "1")
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
  let child = spawn_child_with_etxtbsy_retry(&mut cmd)?;
  wait_with_deadline(child, GH_TIMEOUT)
}

fn spawn_child_with_etxtbsy_retry(
  cmd: &mut std::process::Command,
) -> Result<std::process::Child, String> {
  let mut attempt = 0;
  loop {
    match cmd.spawn() {
      Ok(child) => return Ok(child),
      Err(e) if e.raw_os_error() == Some(ETXTBSY) && attempt < 5 => {
        attempt += 1;
        std::thread::sleep(std::time::Duration::from_millis(20 * attempt as u64));
      }
      Err(e) => return Err(e.to_string()),
    }
  }
}

/// Collects the child's output, killing it if it outlives `timeout`.
fn wait_with_deadline(
  mut child: std::process::Child,
  timeout: std::time::Duration,
) -> Result<std::process::Output, String> {
  use std::io::Read;

  // One thread per pipe so neither fills up and stalls the child.
  fn drain(pipe: Option<impl Read + Send + 'static>) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
      let mut buf = Vec::new();
      if let Some(mut pipe) = pipe {
        pipe.read_to_end(&mut buf).best_effort();
      }
      buf
    })
  }
  let stdout = drain(child.stdout.take());
  let stderr = drain(child.stderr.take());

  let deadline = std::time::Instant::now() + timeout;
  let status = loop {
    if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
      break status;
    }
    if std::time::Instant::now() >= deadline {
      child.kill().best_effort();
      child.wait().best_effort();
      return Err(format!("gh timed out after {}s", timeout.as_secs()));
    }
    std::thread::sleep(std::time::Duration::from_millis(50));
  };
  Ok(std::process::Output {
    status,
    stdout: stdout.join().unwrap_or_default(),
    stderr: stderr.join().unwrap_or_default(),
  })
}

fn run_gh(
  gh_path: &str,
  args: &[&str],
  extended_path: &str,
) -> Result<std::process::Output, String> {
  let mut cmd = std::process::Command::new(gh_path);
  cmd.args(args).env("PATH", extended_path);
  spawn_with_etxtbsy_retry(cmd)
}

/// Run `gh` with `stdin` piped in. Used for `--body-file -` so a body is
/// never an argv value: Linux caps a single argument at 128 KiB, and a body
/// passed on stdin cannot be mistaken for a flag.
fn run_gh_with_stdin(
  gh_path: &str,
  args: &[&str],
  stdin: &str,
  extended_path: &str,
) -> Result<std::process::Output, String> {
  use std::io::Write;
  use std::process::Stdio;

  let mut cmd = std::process::Command::new(gh_path);
  cmd
    .args(args)
    .env("PATH", extended_path)
    .env("GH_PROMPT_DISABLED", "1")
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
  let mut child = spawn_child_with_etxtbsy_retry(&mut cmd)?;
  let mut pipe = child
    .stdin
    .take()
    .ok_or_else(|| "Failed to open gh stdin".to_string())?;
  let data = stdin.as_bytes().to_vec();
  // Separate thread: a large body must not deadlock against gh's output pipes.
  let writer = std::thread::spawn(move || pipe.write_all(&data));
  let output = wait_with_deadline(child, GH_TIMEOUT)?;
  let written = writer
    .join()
    .map_err(|_| "gh stdin writer panicked".to_string())?;
  // Broken pipe: gh exited unread; on failure check_gh_output reports gh's error.
  match written {
    Err(e) if output.status.success() && e.kind() != std::io::ErrorKind::BrokenPipe => {
      Err(format!("Failed to write gh stdin: {e}"))
    }
    _ => Ok(output),
  }
}

pub const GH_NOT_AUTHENTICATED: &str =
  "gh is not authenticated. Run `gh auth login` in a terminal, then try again.";

fn check_gh_output(output: std::process::Output) -> Result<Vec<u8>, String> {
  if output.status.success() {
    Ok(output.stdout)
  } else {
    let stderr = String::from_utf8_lossy(&output.stderr);
    if is_gh_auth_error(&stderr) {
      return Err(GH_NOT_AUTHENTICATED.to_string());
    }
    Err(format!("gh exited with error: {stderr}"))
  }
}

/// gh's messages when it has no usable token: never logged in, or a token
/// GitHub no longer accepts.
fn is_gh_auth_error(stderr: &str) -> bool {
  let stderr = stderr.to_ascii_lowercase();
  stderr.contains("gh auth login")
    || stderr.contains("not logged into any github hosts")
    || stderr.contains("http 401")
}

/// Parse `<n>` from a `gh ... create` output line shaped like
/// `https://github.com/<owner>/<repo>/<kind>/<n>`, where `kind` is `issues` or
/// `pull`. gh can print other lines before the URL (template notices, push
/// output), and any of those can end in a number.
fn parse_number_from_url(line: &str, kind: &str) -> Option<u64> {
  let line = line.trim();
  if !line.starts_with("https://") && !line.starts_with("http://") {
    return None;
  }
  let mut segments = line.rsplit('/');
  let number = segments.next()?.parse().ok()?;
  (segments.next()? == kind).then_some(number)
}

pub const GH_LIST_PAGE_SIZE: u32 = 30;

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GhListPage<T> {
  pub items: Vec<T>,
  pub has_more: bool,
  /// Cursor to pass as `after` for the next page. Only cursor-paged lists
  /// (issues) set it; offset-paged lists (PRs) leave it `None`.
  #[serde(default)]
  pub end_cursor: Option<String>,
}

/// Given items fetched with `--limit (limit * page)`, return the requested page slice.
pub fn take_list_page<T>(fetched: Vec<T>, limit: u32, page: u32) -> GhListPage<T> {
  let limit = limit.max(1) as usize;
  let page = page.max(1) as usize;
  let has_more = fetched.len() == limit * page;
  let start = (page - 1) * limit;
  let items = if start >= fetched.len() {
    Vec::new()
  } else {
    fetched.into_iter().skip(start).take(limit).collect()
  };
  GhListPage {
    items,
    has_more,
    end_cursor: None,
  }
}

/// One page of `repository.issues`, newest first. GitHub's cursor marks a
/// position in the list, so the next page starts after the last issue this
/// page returned even if issues were opened in between. `gh issue list` has
/// no cursor, so this goes through `gh api graphql`.
const ISSUES_PAGE_QUERY: &str = r#"query($owner: String!, $name: String!, $first: Int!, $after: String) {
  repository(owner: $owner, name: $name) {
    issues(first: $first, after: $after, STATES orderBy: {field: CREATED_AT, direction: DESC}) {
      nodes {
        number title state url createdAt updatedAt
        author { login }
        labels(first: 20) { nodes { name color } }
      }
      pageInfo { hasNextPage endCursor }
    }
  }
}"#;

#[derive(serde::Deserialize)]
struct IssuesPageResponse {
  data: IssuesPageData,
}

#[derive(serde::Deserialize)]
struct IssuesPageData {
  repository: IssuesPageRepository,
}

#[derive(serde::Deserialize)]
struct IssuesPageRepository {
  issues: IssuesPageConnection,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct IssuesPageConnection {
  nodes: Vec<IssuesPageNode>,
  page_info: IssuesPageInfo,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct IssuesPageInfo {
  has_next_page: bool,
  end_cursor: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct IssuesPageNode {
  number: u64,
  title: String,
  state: String,
  url: String,
  created_at: String,
  updated_at: String,
  /// Null when the author's account was deleted.
  author: Option<GhAuthor>,
  labels: IssuesPageLabels,
}

#[derive(serde::Deserialize)]
struct IssuesPageLabels {
  nodes: Vec<GhLabel>,
}

impl From<IssuesPageNode> for GhIssue {
  fn from(node: IssuesPageNode) -> Self {
    GhIssue {
      number: node.number,
      title: node.title,
      state: node.state,
      url: node.url,
      body: None,
      // GitHub shows deleted accounts as "ghost"; gh issue list did too.
      author: node.author.unwrap_or_else(|| GhAuthor {
        login: "ghost".to_string(),
        avatar_url: None,
      }),
      labels: node.labels.nodes,
      created_at: node.created_at,
      updated_at: node.updated_at,
      comments: None,
    }
  }
}

pub fn gh_list_issues_impl(
  gh_path: &str,
  repo_full_name: &str,
  state: &str,
  limit: u32,
  after: Option<&str>,
  extended_path: &str,
) -> Result<GhListPage<GhIssue>, String> {
  let (owner, name) = repo_full_name
    .split_once('/')
    .filter(|(owner, name)| !owner.is_empty() && !name.is_empty() && !name.contains('/'))
    .ok_or_else(|| format!("Expected an OWNER/REPO name, got: {repo_full_name}"))?;
  // The state becomes GraphQL text, so only these fixed values reach it.
  let states = match state {
    "open" => "states: [OPEN],",
    "closed" => "states: [CLOSED],",
    "all" => "",
    other => return Err(format!("Unknown issue state: {other}")),
  };
  let query = format!("query={}", ISSUES_PAGE_QUERY.replace("STATES ", states));
  let owner = format!("owner={owner}");
  let name = format!("name={name}");
  let first = format!("first={}", limit.clamp(1, 100));
  let after = after.map(|cursor| format!("after={cursor}"));
  let mut args = vec![
    "api", "graphql", "-f", &query, "-f", &owner, "-f", &name, "-F", &first,
  ];
  if let Some(after) = &after {
    args.extend(["-f", after]);
  }
  let out = run_gh(gh_path, &args, extended_path)?;
  let bytes = check_gh_output(out)?;
  let response: IssuesPageResponse =
    serde_json::from_slice(&bytes).map_err(|e| format!("Failed to parse gh output: {e}"))?;
  let issues = response.data.repository.issues;
  Ok(GhListPage {
    items: issues.nodes.into_iter().map(GhIssue::from).collect(),
    has_more: issues.page_info.has_next_page,
    end_cursor: issues.page_info.end_cursor,
  })
}

pub fn gh_view_issue_impl(
  gh_path: &str,
  repo_full_name: &str,
  issue_number: u64,
  extended_path: &str,
) -> Result<GhIssue, String> {
  let num = issue_number.to_string();
  let out = run_gh(
    gh_path,
    &[
      "issue",
      "view",
      &num,
      "--repo",
      repo_full_name,
      "--json",
      "number,title,state,url,body,author,labels,createdAt,updatedAt,comments",
    ],
    extended_path,
  )?;
  let bytes = check_gh_output(out)?;
  serde_json::from_slice(&bytes).map_err(|e| format!("Failed to parse gh output: {e}"))
}

pub fn gh_create_issue_impl(
  gh_path: &str,
  repo_full_name: &str,
  title: &str,
  body: &str,
  extended_path: &str,
) -> Result<u64, String> {
  let out = run_gh_with_stdin(
    gh_path,
    &[
      "issue",
      "create",
      "--repo",
      repo_full_name,
      "--title",
      title,
      "--body-file",
      "-",
    ],
    body,
    extended_path,
  )?;
  let bytes = check_gh_output(out)?;
  let text = String::from_utf8_lossy(&bytes);
  for line in text.lines() {
    if let Some(n) = parse_number_from_url(line, "issues") {
      return Ok(n);
    }
  }
  Err(format!("Could not parse issue number from output: {text}"))
}

pub fn gh_create_issue_comment_impl(
  gh_path: &str,
  repo_full_name: &str,
  issue_number: u64,
  body: &str,
  extended_path: &str,
) -> Result<(), String> {
  let num = issue_number.to_string();
  let out = run_gh_with_stdin(
    gh_path,
    &[
      "issue",
      "comment",
      &num,
      "--repo",
      repo_full_name,
      "--body-file",
      "-",
    ],
    body,
    extended_path,
  )?;
  check_gh_output(out).map(|_| ())
}

/// Close reasons `gh issue close --reason` accepts.
const ISSUE_CLOSE_REASONS: [&str; 2] = ["completed", "not planned"];

pub fn gh_close_issue_impl(
  gh_path: &str,
  repo_full_name: &str,
  issue_number: u64,
  reason: Option<&str>,
  extended_path: &str,
) -> Result<(), String> {
  let num = issue_number.to_string();
  let mut args = vec!["issue", "close", &num, "--repo", repo_full_name];
  if let Some(reason) = reason {
    if !ISSUE_CLOSE_REASONS.contains(&reason) {
      return Err(format!("Unknown issue close reason: {reason}"));
    }
    args.extend(["--reason", reason]);
  }
  let out = run_gh(gh_path, &args, extended_path)?;
  check_gh_output(out).map(|_| ())
}

/// Replace an issue's title and body. The body goes over stdin, like
/// `gh_create_issue_impl`, so its size and content never hit argv.
pub fn gh_edit_issue_impl(
  gh_path: &str,
  repo_full_name: &str,
  issue_number: u64,
  title: &str,
  body: &str,
  extended_path: &str,
) -> Result<(), String> {
  let num = issue_number.to_string();
  let out = run_gh_with_stdin(
    gh_path,
    &[
      "issue",
      "edit",
      &num,
      "--repo",
      repo_full_name,
      "--title",
      title,
      "--body-file",
      "-",
    ],
    body,
    extended_path,
  )?;
  check_gh_output(out).map(|_| ())
}

/// Replace a PR's title and/or body; a `None` field is left as it is. The
/// body goes over stdin, like [`gh_edit_issue_impl`].
pub fn gh_edit_pr_impl(
  gh_path: &str,
  repo_full_name: &str,
  pr_number: u64,
  title: Option<&str>,
  body: Option<&str>,
  extended_path: &str,
) -> Result<(), String> {
  let num = pr_number.to_string();
  let mut args = vec!["pr", "edit", num.as_str(), "--repo", repo_full_name];
  if let Some(title) = title {
    args.extend(["--title", title]);
  }
  if body.is_some() {
    args.extend(["--body-file", "-"]);
  }
  let out = run_gh_with_stdin(gh_path, &args, body.unwrap_or_default(), extended_path)?;
  check_gh_output(out).map(|_| ())
}

/// Delete an issue. GitHub allows this only for repo admins; gh reports the
/// permission error otherwise. `--yes` skips gh's interactive prompt, so the
/// caller must confirm with the user first.
pub fn gh_delete_issue_impl(
  gh_path: &str,
  repo_full_name: &str,
  issue_number: u64,
  extended_path: &str,
) -> Result<(), String> {
  let num = issue_number.to_string();
  let out = run_gh(
    gh_path,
    &["issue", "delete", &num, "--repo", repo_full_name, "--yes"],
    extended_path,
  )?;
  check_gh_output(out).map(|_| ())
}

pub fn gh_reopen_issue_impl(
  gh_path: &str,
  repo_full_name: &str,
  issue_number: u64,
  extended_path: &str,
) -> Result<(), String> {
  let num = issue_number.to_string();
  let out = run_gh(
    gh_path,
    &["issue", "reopen", &num, "--repo", repo_full_name],
    extended_path,
  )?;
  check_gh_output(out).map(|_| ())
}

pub fn gh_list_prs_impl(
  gh_path: &str,
  repo_full_name: &str,
  state: &str,
  limit: u32,
  page: u32,
  extended_path: &str,
) -> Result<GhListPage<GhPullRequest>, String> {
  let limit = limit.max(1);
  let page = page.max(1);
  let fetch_limit = (limit * page).to_string();
  let is_draft = state == "draft";
  let gh_state = if is_draft { "open" } else { state };
  let mut args = vec!["pr", "list", "--repo", repo_full_name, "--state", gh_state];
  if is_draft {
    args.push("--draft");
  } else if gh_state == "open" {
    // `--state open` includes drafts, which have their own filter.
    args.extend(["--search", "draft:false"]);
  }
  args.extend([
    "--json",
    "number,title,state,url,author,labels,headRefName,baseRefName,mergeStateStatus,createdAt,updatedAt,isDraft",
    "--limit",
    fetch_limit.as_str(),
  ]);
  let out = run_gh(gh_path, &args, extended_path)?;
  let bytes = check_gh_output(out)?;
  let fetched: Vec<GhPullRequest> =
    serde_json::from_slice(&bytes).map_err(|e| format!("Failed to parse gh output: {e}"))?;
  Ok(take_list_page(fetched, limit, page))
}

/// Open pull requests, newest first, drafts included: the viewer's own
/// (`--author @me`) or everyone's, narrowed by `query` (GitHub search syntax).
pub fn gh_search_open_prs_impl(
  gh_path: &str,
  repo_full_name: &str,
  mine: bool,
  query: &str,
  limit: u32,
  extended_path: &str,
) -> Result<Vec<GhPullRequest>, String> {
  let search = format!("{} sort:created-desc", query.trim())
    .trim()
    .to_string();
  let limit = limit.max(1).to_string();
  let mut args = vec![
    "pr",
    "list",
    "--repo",
    repo_full_name,
    "--state",
    "open",
    "--search",
    search.as_str(),
  ];
  if mine {
    args.extend(["--author", "@me"]);
  }
  args.extend([
    "--json",
    "number,title,state,url,author,labels,headRefName,baseRefName,mergeStateStatus,createdAt,updatedAt,isDraft",
    "--limit",
    limit.as_str(),
  ]);
  let out = run_gh(gh_path, &args, extended_path)?;
  let bytes = check_gh_output(out)?;
  serde_json::from_slice(&bytes).map_err(|e| format!("Failed to parse gh output: {e}"))
}

pub fn gh_view_pr_impl(
  gh_path: &str,
  repo_full_name: &str,
  pr_number: u64,
  extended_path: &str,
) -> Result<GhPullRequest, String> {
  let num = pr_number.to_string();
  let out = run_gh(
        gh_path,
        &[
            "pr",
            "view",
            &num,
            "--repo",
            repo_full_name,
            "--json",
            "number,title,state,url,body,author,labels,headRefName,baseRefName,mergeStateStatus,createdAt,updatedAt,comments,isDraft",
        ],
        extended_path,
    )?;
  let bytes = check_gh_output(out)?;
  serde_json::from_slice(&bytes).map_err(|e| format!("Failed to parse gh output: {e}"))
}

pub fn gh_create_pr_comment_impl(
  gh_path: &str,
  repo_full_name: &str,
  pr_number: u64,
  body: &str,
  extended_path: &str,
) -> Result<(), String> {
  let num = pr_number.to_string();
  let out = run_gh_with_stdin(
    gh_path,
    &[
      "pr",
      "comment",
      &num,
      "--repo",
      repo_full_name,
      "--body-file",
      "-",
    ],
    body,
    extended_path,
  )?;
  check_gh_output(out).map(|_| ())
}

/// Login of the user `gh` is authenticated as.
pub fn gh_current_user_impl(gh_path: &str, extended_path: &str) -> Result<String, String> {
  let out = run_gh(gh_path, &["api", "user", "--jq", ".login"], extended_path)?;
  let bytes = check_gh_output(out)?;
  Ok(String::from_utf8_lossy(&bytes).trim().to_string())
}

/// Unified diff of a PR (`gh pr diff`), base...head as GitHub shows it.
pub fn gh_pr_diff_impl(
  gh_path: &str,
  repo_full_name: &str,
  pr_number: u64,
  extended_path: &str,
) -> Result<String, String> {
  let num = pr_number.to_string();
  let out = run_gh(
    gh_path,
    &["pr", "diff", &num, "--repo", repo_full_name],
    extended_path,
  )?;
  let bytes = check_gh_output(out)?;
  Ok(String::from_utf8_lossy(&bytes).into_owned())
}

const REPLY_THREAD_MUTATION: &str = "mutation($threadId: ID!, $body: String!) { addPullRequestReviewThreadReply(input: {pullRequestReviewThreadId: $threadId, body: $body}) { comment { id } } }";

/// `gh api` exits 0 on GraphQL-level errors, so check the `errors` field.
fn check_graphql_errors(bytes: &[u8]) -> Result<(), String> {
  let value: serde_json::Value = serde_json::from_slice(bytes).unwrap_or_default();
  match value.get("errors").and_then(|e| e.as_array()) {
    Some(errors) if !errors.is_empty() => Err(
      errors
        .iter()
        .filter_map(|e| e.get("message").and_then(|m| m.as_str()))
        .collect::<Vec<_>>()
        .join("; "),
    ),
    _ => Ok(()),
  }
}

/// Reply to a PR review thread (GraphQL node id from the review-threads query).
pub fn gh_reply_pr_review_thread_impl(
  gh_path: &str,
  thread_id: &str,
  body: &str,
  extended_path: &str,
) -> Result<(), String> {
  let input = serde_json::json!({
    "query": REPLY_THREAD_MUTATION,
    "variables": { "threadId": thread_id, "body": body },
  });
  let out = run_gh_with_stdin(
    gh_path,
    &["api", "graphql", "--input", "-"],
    &input.to_string(),
    extended_path,
  )?;
  check_graphql_errors(&check_gh_output(out)?)
}

/// GraphQL lists all eight reactions, most with no one on them.
fn shown_reactions(groups: Vec<GhReactionGroup>) -> Vec<GhReactionGroup> {
  groups.into_iter().filter(|g| g.count > 0).collect()
}

const COMMENT_REACTIONS_QUERY: &str = "query($ids: [ID!]!) { nodes(ids: $ids) { ... on Node { id } ... on Reactable { reactionGroups { content viewerHasReacted reactors { totalCount } } } } }";

/// The reactions on comments by GraphQL node id, with whether the viewer
/// reacted, which `gh ... view --json comments` leaves out.
pub fn gh_comment_reactions_impl(
  gh_path: &str,
  ids: &[String],
  extended_path: &str,
) -> Result<std::collections::HashMap<String, Vec<GhReactionGroup>>, String> {
  #[derive(serde::Deserialize)]
  struct Response {
    data: Option<Nodes>,
  }
  #[derive(serde::Deserialize)]
  struct Nodes {
    nodes: Vec<Option<Node>>,
  }
  #[derive(serde::Deserialize)]
  struct Node {
    id: Option<String>,
    #[serde(rename = "reactionGroups", default)]
    reaction_groups: Vec<GhReactionGroup>,
  }

  let mut out = std::collections::HashMap::new();
  // `nodes` takes at most 100 ids.
  for chunk in ids.chunks(100) {
    let input = serde_json::json!({
      "query": COMMENT_REACTIONS_QUERY,
      "variables": { "ids": chunk },
    });
    let out_raw = run_gh_with_stdin(
      gh_path,
      &["api", "graphql", "--input", "-"],
      &input.to_string(),
      extended_path,
    )?;
    let bytes = check_gh_output(out_raw)?;
    check_graphql_errors(&bytes)?;
    let parsed: Response = serde_json::from_slice(&bytes)
      .map_err(|e| format!("Failed to parse gh graphql output: {e}"))?;
    for node in parsed.data.into_iter().flat_map(|d| d.nodes).flatten() {
      if let Some(id) = node.id {
        out.insert(id, shown_reactions(node.reaction_groups));
      }
    }
  }
  Ok(out)
}

/// Fills in `comments`' reactions with whether the viewer reacted. A failed
/// read keeps `gh`'s counts.
pub fn fill_comment_reactions(
  gh_path: &str,
  comments: Option<&mut Vec<GhIssueComment>>,
  extended_path: &str,
) {
  let Some(comments) = comments.filter(|c| !c.is_empty()) else {
    return;
  };
  let ids: Vec<String> = comments.iter().map(|c| c.id.clone()).collect();
  match gh_comment_reactions_impl(gh_path, &ids, extended_path) {
    Ok(mut by_id) => {
      for c in comments.iter_mut() {
        if let Some(reactions) = by_id.remove(&c.id) {
          c.reactions = reactions;
        }
      }
    }
    Err(err) => log::warn!("Failed to read comment reactions: {err}"),
  }
}

const ADD_REACTION_MUTATION: &str = "mutation($id: ID!, $content: ReactionContent!) { addReaction(input: {subjectId: $id, content: $content}) { clientMutationId } }";
const REMOVE_REACTION_MUTATION: &str = "mutation($id: ID!, $content: ReactionContent!) { removeReaction(input: {subjectId: $id, content: $content}) { clientMutationId } }";

/// Adds or removes the viewer's `content` reaction (`THUMBS_UP`, ...) on a
/// comment by its GraphQL node id.
pub fn gh_set_reaction_impl(
  gh_path: &str,
  subject_id: &str,
  content: &str,
  on: bool,
  extended_path: &str,
) -> Result<(), String> {
  let input = serde_json::json!({
    "query": if on { ADD_REACTION_MUTATION } else { REMOVE_REACTION_MUTATION },
    "variables": { "id": subject_id, "content": content },
  });
  let out = run_gh_with_stdin(
    gh_path,
    &["api", "graphql", "--input", "-"],
    &input.to_string(),
    extended_path,
  )?;
  check_graphql_errors(&check_gh_output(out)?)
}

/// One inline comment of a submitted review. `side` is `LEFT` or `RIGHT`;
/// `start_line` makes it a multi-line comment on the same side.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct GhDraftReviewComment {
  pub path: String,
  pub line: u64,
  pub side: String,
  pub start_line: Option<u64>,
  pub body: String,
}

/// Submit a PR review (`APPROVE`, `REQUEST_CHANGES` or `COMMENT`) with its
/// inline comments in one REST call.
pub fn gh_submit_pr_review_impl(
  gh_path: &str,
  repo_full_name: &str,
  pr_number: u64,
  event: &str,
  body: &str,
  comments: &[GhDraftReviewComment],
  extended_path: &str,
) -> Result<(), String> {
  if !matches!(event, "APPROVE" | "REQUEST_CHANGES" | "COMMENT") {
    return Err(format!("Invalid review event: {event}"));
  }
  let comments: Vec<serde_json::Value> = comments
    .iter()
    .map(|c| {
      let mut v = serde_json::json!({
        "path": c.path,
        "line": c.line,
        "side": c.side,
        "body": c.body,
      });
      if let Some(start) = c.start_line.filter(|s| *s < c.line) {
        v["start_line"] = start.into();
        v["start_side"] = c.side.clone().into();
      }
      v
    })
    .collect();
  let input = serde_json::json!({ "event": event, "body": body, "comments": comments });
  let endpoint = format!("repos/{repo_full_name}/pulls/{pr_number}/reviews");
  let out = run_gh_with_stdin(
    gh_path,
    &["api", &endpoint, "--method", "POST", "--input", "-"],
    &input.to_string(),
    extended_path,
  )?;
  check_gh_output(out).map(|_| ())
}

pub fn gh_close_pr_impl(
  gh_path: &str,
  repo_full_name: &str,
  pr_number: u64,
  extended_path: &str,
) -> Result<(), String> {
  let num = pr_number.to_string();
  let out = run_gh(
    gh_path,
    &["pr", "close", &num, "--repo", repo_full_name],
    extended_path,
  )?;
  check_gh_output(out).map(|_| ())
}

pub fn gh_reopen_pr_impl(
  gh_path: &str,
  repo_full_name: &str,
  pr_number: u64,
  extended_path: &str,
) -> Result<(), String> {
  let num = pr_number.to_string();
  let out = run_gh(
    gh_path,
    &["pr", "reopen", &num, "--repo", repo_full_name],
    extended_path,
  )?;
  check_gh_output(out).map(|_| ())
}

/// Mark a PR ready for review (`draft = false`) or convert it to draft (`draft = true`).
pub fn gh_set_pr_draft_impl(
  gh_path: &str,
  repo_full_name: &str,
  pr_number: u64,
  draft: bool,
  extended_path: &str,
) -> Result<(), String> {
  let num = pr_number.to_string();
  let mut args = vec!["pr", "ready", num.as_str(), "--repo", repo_full_name];
  if draft {
    args.push("--undo");
  }
  let out = run_gh(gh_path, &args, extended_path)?;
  check_gh_output(out).map(|_| ())
}

/// How GitHub lands a pull request: `gh pr merge --merge|--squash|--rebase`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GhMergeMethod {
  Merge,
  Squash,
  Rebase,
}

impl GhMergeMethod {
  fn flag(self) -> &'static str {
    match self {
      GhMergeMethod::Merge => "--merge",
      GhMergeMethod::Squash => "--squash",
      GhMergeMethod::Rebase => "--rebase",
    }
  }

  fn from_graphql(value: &str) -> Option<Self> {
    match value {
      "MERGE" => Some(GhMergeMethod::Merge),
      "SQUASH" => Some(GhMergeMethod::Squash),
      "REBASE" => Some(GhMergeMethod::Rebase),
      _ => None,
    }
  }
}

/// Merge methods a repository allows, and the one GitHub preselects for the viewer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GhRepoMergeSettings {
  pub default_method: GhMergeMethod,
  pub allowed_methods: Vec<GhMergeMethod>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhRepoMergeSettingsRaw {
  viewer_default_merge_method: Option<String>,
  merge_commit_allowed: bool,
  squash_merge_allowed: bool,
  rebase_merge_allowed: bool,
}

pub fn gh_repo_merge_settings_impl(
  gh_path: &str,
  repo_full_name: &str,
  extended_path: &str,
) -> Result<GhRepoMergeSettings, String> {
  let out = run_gh(
    gh_path,
    &[
      "repo",
      "view",
      repo_full_name,
      "--json",
      "viewerDefaultMergeMethod,mergeCommitAllowed,squashMergeAllowed,rebaseMergeAllowed",
    ],
    extended_path,
  )?;
  let bytes = check_gh_output(out)?;
  let raw: GhRepoMergeSettingsRaw =
    serde_json::from_slice(&bytes).map_err(|e| format!("Failed to parse gh output: {e}"))?;
  let allowed_methods: Vec<GhMergeMethod> = [
    (raw.merge_commit_allowed, GhMergeMethod::Merge),
    (raw.squash_merge_allowed, GhMergeMethod::Squash),
    (raw.rebase_merge_allowed, GhMergeMethod::Rebase),
  ]
  .into_iter()
  .filter_map(|(allowed, method)| allowed.then_some(method))
  .collect();
  let default_method = raw
    .viewer_default_merge_method
    .as_deref()
    .and_then(GhMergeMethod::from_graphql)
    .filter(|method| allowed_methods.contains(method))
    .or_else(|| allowed_methods.first().copied())
    .ok_or_else(|| format!("{repo_full_name} allows no merge methods"))?;
  Ok(GhRepoMergeSettings {
    default_method,
    allowed_methods,
  })
}

/// Turn `gh pr merge` stderr into a message that names the blocker.
/// The raw gh output is kept after the summary so nothing is lost.
pub fn classify_gh_merge_error(stderr: &str) -> String {
  let lower = stderr.to_lowercase();
  let summary =
    if lower.contains("status check") || (lower.contains("checks") && lower.contains("fail")) {
      "Required checks are failing"
    } else if lower.contains("conflict") || lower.contains("cannot be cleanly created") {
      "The pull request has merge conflicts"
    } else if lower.contains("not allowed on this repository") {
      "This merge method is not allowed on this repository"
    } else if lower.contains("review") && lower.contains("required") {
      "The pull request needs an approving review"
    } else if lower.contains("not mergeable") {
      "The pull request is not mergeable"
    } else {
      return format!("GitHub merge failed: {}", stderr.trim());
    };
  format!("{summary}: {}", stderr.trim())
}

/// Merge a PR on GitHub. Rebase merges keep the original commit messages, so
/// `subject`/`body` only apply to merge and squash.
pub fn gh_merge_pr_impl(
  gh_path: &str,
  repo_full_name: &str,
  pr_number: u64,
  method: GhMergeMethod,
  subject: Option<&str>,
  body: Option<&str>,
  extended_path: &str,
) -> Result<(), String> {
  let num = pr_number.to_string();
  let mut args = vec![
    "pr",
    "merge",
    num.as_str(),
    "--repo",
    repo_full_name,
    method.flag(),
  ];
  let (subject, body) = match method {
    GhMergeMethod::Rebase => (None, None),
    _ => (
      subject.filter(|s| !s.trim().is_empty()),
      body.filter(|b| !b.trim().is_empty()),
    ),
  };
  if let Some(subject) = subject {
    args.extend(["--subject", subject]);
  }
  let out = match body {
    Some(body) => {
      args.extend(["--body-file", "-"]);
      run_gh_with_stdin(gh_path, &args, body, extended_path)?
    }
    None => run_gh(gh_path, &args, extended_path)?,
  };
  if out.status.success() {
    Ok(())
  } else {
    Err(classify_gh_merge_error(&String::from_utf8_lossy(
      &out.stderr,
    )))
  }
}

pub fn gh_set_pr_base_impl(
  gh_path: &str,
  repo_full_name: &str,
  pr_number: u64,
  base_branch: &str,
  extended_path: &str,
) -> Result<(), String> {
  let num = pr_number.to_string();
  let out = run_gh(
    gh_path,
    &[
      "pr",
      "edit",
      &num,
      "--repo",
      repo_full_name,
      "--base",
      base_branch,
    ],
    extended_path,
  )?;
  check_gh_output(out).map(|_| ())
}

/// Point the open PR of each `(branch, new_base)` at its new base, so stacked
/// PRs follow their workspace after the parent lands. Best-effort: returns one
/// note per PR moved or failed; branches without an open PR are skipped.
pub fn gh_retarget_open_prs_impl(
  gh_path: &str,
  repo_path: &str,
  repo_full_name: &str,
  retargets: &[(String, String)],
  extended_path: &str,
) -> Vec<String> {
  retargets
    .iter()
    .filter_map(|(branch, new_base)| {
      let pr = get_pr_info_via_gh_impl(gh_path, repo_path, branch, extended_path)
        .ok()
        .flatten()
        .filter(|pr| pr.state == "OPEN" && pr.base_ref_name != *new_base)?;
      Some(
        match gh_set_pr_base_impl(gh_path, repo_full_name, pr.number, new_base, extended_path) {
          Ok(()) => format!("Moved PR #{} ({}) onto {}", pr.number, branch, new_base),
          Err(e) => format!(
            "Could not move PR #{} ({}) onto {}: {}",
            pr.number, branch, new_base, e
          ),
        },
      )
    })
    .collect()
}

#[expect(
  clippy::too_many_arguments,
  reason = "flat argument list kept to match callers"
)]
pub fn gh_create_pr_impl(
  gh_path: &str,
  repo_full_name: &str,
  title: &str,
  body: &str,
  base_branch: &str,
  head_branch: &str,
  draft: bool,
  extended_path: &str,
) -> Result<u64, String> {
  let mut args = vec![
    "pr",
    "create",
    "--repo",
    repo_full_name,
    "--title",
    title,
    "--body-file",
    "-",
    "--base",
    base_branch,
    "--head",
    head_branch,
  ];
  if draft {
    args.push("--draft");
  }
  let out = run_gh_with_stdin(gh_path, &args, body, extended_path)?;
  let bytes = check_gh_output(out)?;
  let text = String::from_utf8_lossy(&bytes);
  for line in text.lines() {
    if let Some(n) = parse_number_from_url(line, "pull") {
      return Ok(n);
    }
  }
  Err(format!("Could not parse PR number from output: {text}"))
}

const CHECK_JSON_FIELDS: &str = "name,bucket,link,startedAt,completedAt";

/// Compute elapsed seconds for a check run from `gh pr checks` timestamps.
/// A run that has not finished has none: `gh` reports its `completedAt` as
/// missing or as Go's zero time, and the time since it started would be stale
/// by the next poll. Inverted timestamps (seen on some skipped checks) clamp
/// to zero.
fn check_duration_secs(started_at: Option<&str>, completed_at: Option<&str>) -> Option<u64> {
  let parse = |raw: &str| {
    chrono::DateTime::parse_from_rfc3339(raw)
      .ok()
      .filter(|t| t.timestamp() > 0)
  };
  let started = parse(started_at?)?;
  let end = parse(completed_at?)?;
  Some(end.signed_duration_since(started).num_seconds().max(0) as u64)
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PrCheckEntry {
  pub name: String,
  /// "pass" | "fail" | "pending" | "skipping" | "cancel"
  pub bucket: String,
  pub link: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub duration_secs: Option<u64>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PrCiStatus {
  /// "success" | "failure" | "pending"
  pub state: String,
  pub total: u32,
  pub passed: u32,
  pub failed: u32,
  pub pending: u32,
  pub checks: Vec<PrCheckEntry>,
}

#[derive(serde::Deserialize)]
struct GhCheck {
  name: String,
  bucket: String,
  link: String,
  #[serde(default, rename = "startedAt")]
  started_at: Option<String>,
  #[serde(default, rename = "completedAt")]
  completed_at: Option<String>,
}

/// Rolls a `gh pr checks --json name,bucket,link,startedAt,completedAt` result
/// up into an overall CI status. Shared by every entry point that surfaces PR
/// CI status (the workspace header's live indicator and the GitHub panel's PR
/// detail view), so they always agree on pass/fail/pending grouping.
fn rollup_pr_checks(raw: Vec<GhCheck>) -> Option<PrCiStatus> {
  if raw.is_empty() {
    return None;
  }

  let mut passed = 0u32;
  let mut failed = 0u32;
  let mut pending = 0u32;
  let checks: Vec<PrCheckEntry> = raw
    .into_iter()
    .map(|c| {
      match c.bucket.as_str() {
        "pass" | "skipping" => passed += 1,
        "fail" | "cancel" => failed += 1,
        _ => pending += 1,
      }
      let duration_secs = check_duration_secs(c.started_at.as_deref(), c.completed_at.as_deref());
      PrCheckEntry {
        name: c.name,
        bucket: c.bucket,
        link: c.link,
        duration_secs,
      }
    })
    .collect();

  let state = if failed > 0 {
    "failure"
  } else if pending > 0 {
    "pending"
  } else {
    "success"
  }
  .to_string();

  Some(PrCiStatus {
    state,
    total: checks.len() as u32,
    passed,
    failed,
    pending,
    checks,
  })
}

/// Run `gh pr checks` for the given branch in the given repo directory and
/// roll the individual check runs up into an overall CI status.
/// Returns None if there is no PR or the PR has no checks reported yet.
/// Returns Err if gh is missing, not authenticated, or fails otherwise.
pub fn get_pr_checks_via_gh_impl(
  gh_path: &str,
  repo_path: &str,
  branch_name: &str,
  extended_path: &str,
) -> Result<Option<PrCiStatus>, String> {
  let mut cmd = std::process::Command::new(gh_path);
  cmd
    // `--` keeps a branch name that starts with `-` from being read as a flag.
    .args([
      "pr",
      "checks",
      "--json",
      CHECK_JSON_FIELDS,
      "--",
      branch_name,
    ])
    .current_dir(repo_path)
    .env("PATH", extended_path);
  let output = spawn_with_etxtbsy_retry(cmd)?;

  if !output.status.success() {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let lower = stderr.to_ascii_lowercase();
    if lower.contains("no pull requests found") || lower.contains("no checks reported") {
      return Ok(None);
    }
    return Err(format!("gh pr checks failed: {}", stderr.trim()));
  }

  let raw: Vec<GhCheck> = serde_json::from_slice(&output.stdout)
    .map_err(|e| format!("Failed to parse gh output: {e}"))?;

  Ok(rollup_pr_checks(raw))
}

/// Run `gh pr checks` for a specific PR number in a repo (identified by
/// `owner/repo`, not a local checkout), for callers -- like the GitHub
/// panel's PR detail view -- that browse PRs without necessarily having a
/// local workspace on that branch. Shares the same rollup as
/// `get_pr_checks_via_gh_impl` so both surfaces always report the same
/// pass/fail/pending grouping for a given PR.
pub fn get_pr_checks_for_pr_impl(
  gh_path: &str,
  repo_full_name: &str,
  pr_number: u64,
  extended_path: &str,
) -> Result<Option<PrCiStatus>, String> {
  let num = pr_number.to_string();
  let out = run_gh(
    gh_path,
    &[
      "pr",
      "checks",
      &num,
      "--repo",
      repo_full_name,
      "--json",
      CHECK_JSON_FIELDS,
    ],
    extended_path,
  )?;

  if !out.status.success() {
    let stderr = String::from_utf8_lossy(&out.stderr);
    let lower = stderr.to_ascii_lowercase();
    if lower.contains("no pull requests found") || lower.contains("no checks reported") {
      return Ok(None);
    }
    return Err(format!("gh pr checks failed: {}", stderr.trim()));
  }

  let raw: Vec<GhCheck> =
    serde_json::from_slice(&out.stdout).map_err(|e| format!("Failed to parse gh output: {e}"))?;

  Ok(rollup_pr_checks(raw))
}

/// Run `gh pr view` for the given branch in the given repo directory.
/// Returns None if no PR exists for the branch. Returns Err if gh is
/// missing, not authenticated, or fails otherwise.
/// The `gh_path` argument is the resolved path to the gh binary.
pub fn get_pr_info_via_gh_impl(
  gh_path: &str,
  repo_path: &str,
  branch_name: &str,
  extended_path: &str,
) -> Result<Option<PrInfo>, String> {
  #[derive(serde::Deserialize)]
  #[serde(rename_all = "camelCase")]
  struct GhPrView {
    number: u64,
    title: String,
    state: String,
    url: String,
    head_ref_name: String,
    base_ref_name: String,
    merge_state_status: Option<String>,
    #[serde(default)]
    is_draft: bool,
    #[serde(default)]
    body: String,
  }

  let mut cmd = std::process::Command::new(gh_path);
  cmd
    // `--` keeps a branch name that starts with `-` from being read as a flag.
    .args([
      "pr",
      "view",
      "--json",
      "number,title,state,url,headRefName,baseRefName,mergeStateStatus,isDraft,body",
      "--",
      branch_name,
    ])
    .current_dir(repo_path)
    .env("PATH", extended_path);
  let output = spawn_with_etxtbsy_retry(cmd)?;

  if !output.status.success() {
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr
      .to_ascii_lowercase()
      .contains("no pull requests found")
    {
      return Ok(None);
    }
    return Err(format!("gh pr view failed: {}", stderr.trim()));
  }

  let raw: GhPrView = serde_json::from_slice(&output.stdout)
    .map_err(|e| format!("Failed to parse gh output: {e}"))?;

  Ok(Some(PrInfo {
    number: raw.number,
    title: raw.title,
    state: raw.state,
    url: raw.url,
    head_ref_name: raw.head_ref_name,
    base_ref_name: raw.base_ref_name,
    merge_state_status: raw.merge_state_status,
    is_draft: raw.is_draft,
    body: raw.body,
  }))
}

/// Newest PRs `gh pr list` returns in one call. A repo with more PRs than this
/// may have a branch's PR beyond the page; see [`PrListing::truncated`].
const PR_LIST_LIMIT: usize = 100;

const PR_LIST_JSON_FIELDS: &str = "number,title,state,url,headRefName,baseRefName,mergeStateStatus,isDraft,isCrossRepository,statusCheckRollup,body";

/// One `statusCheckRollup` entry: a check run or a commit status.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhRollupEntry {
  #[serde(default)]
  name: Option<String>,
  #[serde(default)]
  context: Option<String>,
  #[serde(default)]
  status: Option<String>,
  #[serde(default)]
  conclusion: Option<String>,
  #[serde(default)]
  state: Option<String>,
  #[serde(default)]
  details_url: Option<String>,
  #[serde(default)]
  target_url: Option<String>,
  #[serde(default)]
  started_at: Option<String>,
  #[serde(default)]
  completed_at: Option<String>,
}

impl GhRollupEntry {
  /// The `bucket` `gh pr checks` reports for this entry.
  fn bucket(&self) -> &'static str {
    let state = match self.status.as_deref() {
      // A check run reports its result in `conclusion` once completed.
      Some("COMPLETED") => self.conclusion.as_deref(),
      Some(status) if !status.is_empty() => Some(status),
      // A commit status carries only `state`.
      _ => self.state.as_deref(),
    };
    match state.unwrap_or_default() {
      "SUCCESS" => "pass",
      "SKIPPED" | "NEUTRAL" => "skipping",
      "ERROR" | "FAILURE" | "STARTUP_FAILURE" | "TIMED_OUT" | "ACTION_REQUIRED" => "fail",
      "CANCELLED" => "cancel",
      _ => "pending",
    }
  }

  fn into_check(self) -> GhCheck {
    let bucket = self.bucket().to_string();
    GhCheck {
      name: self.name.or(self.context).unwrap_or_default(),
      bucket,
      link: self.details_url.or(self.target_url).unwrap_or_default(),
      started_at: self.started_at,
      completed_at: self.completed_at,
    }
  }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhListedPr {
  number: u64,
  title: String,
  state: String,
  url: String,
  head_ref_name: String,
  base_ref_name: String,
  merge_state_status: Option<String>,
  #[serde(default)]
  is_draft: bool,
  #[serde(default)]
  is_cross_repository: bool,
  #[serde(default)]
  status_check_rollup: Option<Vec<GhRollupEntry>>,
  #[serde(default)]
  body: String,
}

/// The PR and rolled-up CI status of each head branch in a `gh pr list` page.
pub struct PrListing {
  pub by_branch: HashMap<String, (PrInfo, Option<PrCiStatus>)>,
  /// The page was full, so a branch missing from it may still have an older PR.
  pub truncated: bool,
}

/// Parses `gh pr list --json` output into one PR per head branch, picking the
/// PR `gh pr view <branch>` would: an open one, else the newest. Fork PRs are
/// skipped because `gh pr view` only matches the repository's own branches.
fn parse_pr_list(stdout: &[u8]) -> Result<PrListing, String> {
  let raw: Vec<GhListedPr> =
    serde_json::from_slice(stdout).map_err(|e| format!("Failed to parse gh output: {e}"))?;
  let truncated = raw.len() >= PR_LIST_LIMIT;
  let mut by_branch: HashMap<String, GhListedPr> = HashMap::new();
  for pr in raw.into_iter().filter(|pr| !pr.is_cross_repository) {
    let rank = |pr: &GhListedPr| (pr.state == "OPEN", pr.number);
    match by_branch.get(&pr.head_ref_name) {
      Some(current) if rank(current) >= rank(&pr) => {}
      _ => {
        by_branch.insert(pr.head_ref_name.clone(), pr);
      }
    }
  }
  let by_branch = by_branch
    .into_iter()
    .map(|(branch, pr)| {
      let ci = rollup_pr_checks(
        pr.status_check_rollup
          .unwrap_or_default()
          .into_iter()
          .map(GhRollupEntry::into_check)
          .collect(),
      );
      let info = PrInfo {
        number: pr.number,
        title: pr.title,
        state: pr.state,
        url: pr.url,
        head_ref_name: pr.head_ref_name,
        base_ref_name: pr.base_ref_name,
        merge_state_status: pr.merge_state_status,
        is_draft: pr.is_draft,
        body: pr.body,
      };
      (branch, (info, ci))
    })
    .collect();
  Ok(PrListing {
    by_branch,
    truncated,
  })
}

/// Lists the repo's newest PRs with their CI rollups in one `gh pr list`,
/// replacing a `gh pr view` + `gh pr checks` pair per branch.
pub fn list_prs_with_checks_via_gh_impl(
  gh_path: &str,
  repo_path: &str,
  extended_path: &str,
) -> Result<PrListing, String> {
  let mut cmd = std::process::Command::new(gh_path);
  cmd
    .args([
      "pr",
      "list",
      "--state",
      "all",
      "--limit",
      &PR_LIST_LIMIT.to_string(),
      "--json",
      PR_LIST_JSON_FIELDS,
    ])
    .current_dir(repo_path)
    .env("PATH", extended_path);
  let output = spawn_with_etxtbsy_retry(cmd)?;
  if !output.status.success() {
    return Err(format!(
      "gh pr list failed: {}",
      String::from_utf8_lossy(&output.stderr).trim()
    ));
  }
  parse_pr_list(&output.stdout)
}

// ── GitHub PR review comment threads (read-only) ────────────────────────────

#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq)]
pub struct GhReviewComment {
  pub id: String,
  pub body: String,
  pub author: GhAuthor,
  pub created_at: String,
  pub diff_hunk: String,
  pub url: String,
  pub reactions: Vec<GhReactionGroup>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq)]
pub struct GhReviewThread {
  pub id: String,
  pub is_resolved: bool,
  pub is_outdated: bool,
  pub path: String,
  pub line: Option<i64>,
  pub diff_side: String,
  pub comments: Vec<GhReviewComment>,
}

const REVIEW_THREADS_QUERY: &str = r#"query($owner: String!, $repo: String!, $number: Int!, $after: String) {
  repository(owner: $owner, name: $repo) {
    pullRequest(number: $number) {
      reviewThreads(first: 50, after: $after) {
        pageInfo { hasNextPage endCursor }
        nodes {
          id
          isResolved
          isOutdated
          path
          line
          diffSide
          comments(first: 50) {
            nodes {
              id
              body
              createdAt
              url
              diffHunk
              author { login avatarUrl }
              reactionGroups { content viewerHasReacted reactors { totalCount } }
            }
          }
        }
      }
    }
  }
}"#;

/// Maximum pages of review threads to fetch (50 threads/page) -- generous for
/// any real PR while bounding worst-case `gh` invocations.
const MAX_REVIEW_THREAD_PAGES: usize = 10;

#[derive(serde::Deserialize)]
struct GqlResponse {
  data: Option<GqlData>,
}

#[derive(serde::Deserialize)]
struct GqlData {
  repository: Option<GqlRepository>,
}

#[derive(serde::Deserialize)]
struct GqlRepository {
  #[serde(rename = "pullRequest")]
  pull_request: Option<GqlPullRequest>,
}

#[derive(serde::Deserialize)]
struct GqlPullRequest {
  #[serde(rename = "reviewThreads")]
  review_threads: GqlReviewThreadsConnection,
}

#[derive(serde::Deserialize)]
struct GqlReviewThreadsConnection {
  #[serde(rename = "pageInfo")]
  page_info: GqlPageInfo,
  nodes: Vec<GqlReviewThread>,
}

#[derive(serde::Deserialize)]
struct GqlPageInfo {
  #[serde(rename = "hasNextPage")]
  has_next_page: bool,
  #[serde(rename = "endCursor")]
  end_cursor: Option<String>,
}

#[derive(serde::Deserialize)]
struct GqlReviewThread {
  id: String,
  #[serde(rename = "isResolved")]
  is_resolved: bool,
  #[serde(rename = "isOutdated")]
  is_outdated: bool,
  path: String,
  line: Option<i64>,
  #[serde(rename = "diffSide")]
  diff_side: String,
  comments: GqlCommentsConnection,
}

#[derive(serde::Deserialize)]
struct GqlCommentsConnection {
  nodes: Vec<GqlComment>,
}

#[derive(serde::Deserialize)]
struct GqlComment {
  id: String,
  body: String,
  #[serde(rename = "createdAt")]
  created_at: String,
  url: String,
  #[serde(rename = "diffHunk")]
  diff_hunk: String,
  author: Option<GqlAuthor>,
  #[serde(rename = "reactionGroups", default)]
  reaction_groups: Vec<GhReactionGroup>,
}

#[derive(serde::Deserialize)]
struct GqlAuthor {
  login: String,
  #[serde(rename = "avatarUrl")]
  avatar_url: Option<String>,
}

/// Fetch every review-comment thread (with resolved/outdated state) on a PR via
/// `gh api graphql`, the only way to get thread-grouped comments with a
/// `isResolved` flag -- the REST/`gh pr view` helpers above don't expose it.
/// Read-only: no mutations are issued.
pub fn gh_list_pr_review_threads_impl(
  gh_path: &str,
  owner: &str,
  repo: &str,
  pr_number: u64,
  extended_path: &str,
) -> Result<Vec<GhReviewThread>, String> {
  let number = pr_number.to_string();
  let mut threads: Vec<GhReviewThread> = Vec::new();
  let mut after: Option<String> = None;

  for _ in 0..MAX_REVIEW_THREAD_PAGES {
    let query_arg = format!("query={REVIEW_THREADS_QUERY}");
    let owner_arg = format!("owner={owner}");
    let repo_arg = format!("repo={repo}");
    let number_arg = format!("number={number}");
    let after_arg = after.as_ref().map(|cursor| format!("after={cursor}"));

    let mut args: Vec<&str> = vec![
      "api",
      "graphql",
      "-f",
      &query_arg,
      "-f",
      &owner_arg,
      "-f",
      &repo_arg,
      "-F",
      &number_arg,
    ];
    if let Some(after_arg) = &after_arg {
      args.push("-f");
      args.push(after_arg);
    }

    let out = run_gh(gh_path, &args, extended_path)?;
    let bytes = check_gh_output(out)?;
    let parsed: GqlResponse = serde_json::from_slice(&bytes)
      .map_err(|e| format!("Failed to parse gh graphql output: {e}"))?;

    let connection = parsed
      .data
      .and_then(|d| d.repository)
      .and_then(|r| r.pull_request)
      .map(|pr| pr.review_threads);

    let Some(connection) = connection else {
      break;
    };

    for node in connection.nodes {
      threads.push(GhReviewThread {
        id: node.id,
        is_resolved: node.is_resolved,
        is_outdated: node.is_outdated,
        path: node.path,
        line: node.line,
        diff_side: node.diff_side,
        comments: node
          .comments
          .nodes
          .into_iter()
          .map(|c| GhReviewComment {
            id: c.id,
            body: c.body,
            author: c
              .author
              .map(|a| GhAuthor {
                login: a.login,
                avatar_url: a.avatar_url,
              })
              .unwrap_or_else(|| GhAuthor {
                login: "ghost".to_string(),
                avatar_url: None,
              }),
            created_at: c.created_at,
            diff_hunk: c.diff_hunk,
            url: c.url,
            reactions: shown_reactions(c.reaction_groups),
          })
          .collect(),
      });
    }

    if !connection.page_info.has_next_page {
      break;
    }
    after = connection.page_info.end_cursor;
    if after.is_none() {
      break;
    }
  }

  Ok(threads)
}

#[cfg(test)]
mod tests {
  #[cfg(unix)]
  #[test]
  fn reports_an_unauthenticated_gh_apart_from_other_failures() {
    use std::os::unix::process::ExitStatusExt;
    let failed = |stderr: &str| std::process::Output {
      status: std::process::ExitStatus::from_raw(1 << 8),
      stdout: Vec::new(),
      stderr: stderr.as_bytes().to_vec(),
    };
    for stderr in [
      "To get started with GitHub CLI, please run:  gh auth login\n",
      "You are not logged into any GitHub hosts. To log in, run: gh auth login\n",
      "HTTP 401: Bad credentials (https://api.github.com/graphql)\n",
    ] {
      assert_eq!(
        super::check_gh_output(failed(stderr)),
        Err(super::GH_NOT_AUTHENTICATED.to_string())
      );
    }
    assert_eq!(
      super::check_gh_output(failed("HTTP 404: Not Found")),
      Err("gh exited with error: HTTP 404: Not Found".to_string())
    );
  }

  fn listed_pr(number: u64, branch: &str, state: &str, cross_repo: bool, rollup: &str) -> String {
    format!(
      r#"{{"number":{number},"title":"t{number}","state":"{state}","url":"u{number}","headRefName":"{branch}","baseRefName":"main","mergeStateStatus":"CLEAN","isDraft":false,"isCrossRepository":{cross_repo},"statusCheckRollup":{rollup}}}"#
    )
  }

  #[test]
  fn pr_list_prefers_open_then_newest_and_skips_forks() {
    let json = format!(
      "[{}]",
      [
        listed_pr(9, "feat/a", "MERGED", false, "[]"),
        listed_pr(7, "feat/a", "OPEN", false, "[]"),
        listed_pr(5, "feat/a", "OPEN", false, "[]"),
        listed_pr(8, "feat/b", "CLOSED", false, "[]"),
        listed_pr(6, "feat/b", "MERGED", false, "[]"),
        listed_pr(10, "feat/c", "OPEN", true, "[]"),
      ]
      .join(",")
    );
    let listing = super::parse_pr_list(json.as_bytes()).unwrap();
    assert!(!listing.truncated);
    assert_eq!(listing.by_branch["feat/a"].0.number, 7);
    assert_eq!(listing.by_branch["feat/b"].0.number, 8);
    assert!(!listing.by_branch.contains_key("feat/c"));
    assert_eq!(listing.by_branch["feat/a"].1, None);
  }

  #[test]
  fn pr_list_rolls_up_check_runs_and_statuses_like_gh_pr_checks() {
    let rollup = r#"[
      {"__typename":"CheckRun","name":"build","status":"COMPLETED","conclusion":"SUCCESS","detailsUrl":"https://x/1","startedAt":"2026-01-01T00:00:00Z","completedAt":"2026-01-01T00:01:00Z"},
      {"__typename":"CheckRun","name":"lint","status":"IN_PROGRESS","conclusion":"","detailsUrl":"https://x/2","startedAt":"2026-01-01T00:00:00Z","completedAt":"0001-01-01T00:00:00Z"},
      {"__typename":"CheckRun","name":"docs","status":"COMPLETED","conclusion":"SKIPPED","detailsUrl":"https://x/3"},
      {"__typename":"StatusContext","context":"ci/legacy","state":"FAILURE","targetUrl":"https://x/4"}
    ]"#;
    let json = format!("[{}]", listed_pr(1, "feat/a", "OPEN", false, rollup));
    let listing = super::parse_pr_list(json.as_bytes()).unwrap();
    let ci = listing.by_branch["feat/a"].1.clone().unwrap();
    assert_eq!(ci.state, "failure");
    assert_eq!((ci.total, ci.passed, ci.failed, ci.pending), (4, 2, 1, 1));
    let buckets: Vec<_> = ci.checks.iter().map(|c| c.bucket.as_str()).collect();
    assert_eq!(buckets, ["pass", "pending", "skipping", "fail"]);
    assert_eq!(ci.checks[0].duration_secs, Some(60));
    assert_eq!(ci.checks[1].duration_secs, None);
    assert_eq!(ci.checks[3].name, "ci/legacy");
    assert_eq!(ci.checks[3].link, "https://x/4");
  }

  #[test]
  fn pr_list_carries_the_pr_body() {
    let json = r#"[{"number":1,"title":"t","state":"OPEN","url":"u","headRefName":"feat/a","baseRefName":"main","mergeStateStatus":"CLEAN","isDraft":false,"isCrossRepository":false,"statusCheckRollup":[],"body":"**Why**"}]"#;
    let listing = super::parse_pr_list(json.as_bytes()).unwrap();
    assert_eq!(listing.by_branch["feat/a"].0.body, "**Why**");
  }

  #[test]
  fn pr_list_marks_a_full_page_as_truncated() {
    let json = format!(
      "[{}]",
      (0..super::PR_LIST_LIMIT as u64)
        .map(|n| listed_pr(n, &format!("b{n}"), "OPEN", false, "null"))
        .collect::<Vec<_>>()
        .join(",")
    );
    assert!(super::parse_pr_list(json.as_bytes()).unwrap().truncated);
  }

  #[cfg(unix)]
  #[test]
  fn gh_that_hangs_is_killed_at_the_deadline() {
    let child = std::process::Command::new("sleep")
      .arg("30")
      .stdout(std::process::Stdio::piped())
      .stderr(std::process::Stdio::piped())
      .spawn()
      .expect("spawn sleep");
    let started = std::time::Instant::now();

    let err = super::wait_with_deadline(child, std::time::Duration::from_millis(200)).unwrap_err();

    assert!(err.contains("timed out"), "{err}");
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
  }

  #[cfg(unix)]
  #[test]
  fn wait_with_deadline_returns_output_of_a_quick_command() {
    let child = std::process::Command::new("sh")
      .args(["-c", "echo out; echo err >&2; exit 3"])
      .stdout(std::process::Stdio::piped())
      .stderr(std::process::Stdio::piped())
      .spawn()
      .expect("spawn sh");

    let output = super::wait_with_deadline(child, std::time::Duration::from_secs(10)).unwrap();

    assert_eq!(output.stdout, b"out\n");
    assert_eq!(output.stderr, b"err\n");
    assert_eq!(output.status.code(), Some(3));
  }

  use super::*;

  /// The host sends what it read from `gh` to the phone as JSON; reading
  /// that JSON back must give the same values.
  #[test]
  fn github_types_survive_a_serialize_deserialize_round_trip() {
    fn round_trip<T: serde::Serialize + serde::de::DeserializeOwned>(
      value: &T,
    ) -> serde_json::Value {
      let json = serde_json::to_value(value).unwrap();
      let back: T = serde_json::from_value(json.clone()).unwrap();
      assert_eq!(serde_json::to_value(&back).unwrap(), json);
      json
    }
    let author = GhAuthor {
      login: "octo".into(),
      avatar_url: Some("https://a".into()),
    };
    let comment = GhIssueComment {
      id: "c1".into(),
      body: "hi".into(),
      author: author.clone(),
      created_at: "2026-01-01T00:00:00Z".into(),
      reactions: vec![GhReactionGroup {
        content: "HEART".into(),
        count: 3,
        viewer_has_reacted: true,
      }],
    };
    round_trip(&GhIssue {
      number: 1,
      title: "t".into(),
      state: "OPEN".into(),
      url: "u".into(),
      body: Some("b".into()),
      author: author.clone(),
      labels: vec![GhLabel {
        name: "bug".into(),
        color: "f00".into(),
      }],
      created_at: "c".into(),
      updated_at: "u".into(),
      comments: Some(vec![comment.clone()]),
    });
    round_trip(&GhListPage {
      items: vec![GhPullRequest {
        number: 2,
        title: "t".into(),
        state: "OPEN".into(),
        url: "u".into(),
        body: None,
        author: author.clone(),
        labels: vec![],
        head_ref_name: "feat".into(),
        base_ref_name: "main".into(),
        merge_state_status: Some("CLEAN".into()),
        created_at: "c".into(),
        updated_at: "u".into(),
        comments: Some(vec![comment]),
        is_draft: true,
      }],
      has_more: true,
      end_cursor: Some("x".into()),
    });
    round_trip(&vec![GhReviewThread {
      id: "t1".into(),
      is_resolved: true,
      is_outdated: false,
      path: "a.rs".into(),
      line: Some(4),
      diff_side: "RIGHT".into(),
      comments: vec![GhReviewComment {
        id: "r1".into(),
        body: "b".into(),
        author,
        created_at: "c".into(),
        diff_hunk: "@@".into(),
        url: "u".into(),
        reactions: vec![],
      }],
    }]);
  }
  use std::fs;
  use std::io::Write;
  use tempfile::TempDir;

  // ── parse_github_remote ──────────────────────────────────────────────────

  #[test]
  fn test_parse_ssh_url() {
    let info = parse_github_remote("git@github.com:owner/repo.git").unwrap();
    assert_eq!(info.owner, "owner");
    assert_eq!(info.repo, "repo");
    assert_eq!(info.full_name, "owner/repo");
  }

  #[test]
  fn test_parse_https_url() {
    let info = parse_github_remote("https://github.com/owner/repo.git").unwrap();
    assert_eq!(info.owner, "owner");
    assert_eq!(info.repo, "repo");
    assert_eq!(info.full_name, "owner/repo");
  }

  #[test]
  fn test_parse_https_url_no_dot_git() {
    let info = parse_github_remote("https://github.com/owner/repo").unwrap();
    assert_eq!(info.full_name, "owner/repo");
  }

  #[test]
  fn test_parse_non_github_url_returns_none() {
    assert!(parse_github_remote("https://gitlab.com/owner/repo.git").is_none());
  }

  #[test]
  fn test_parse_invalid_url_returns_none() {
    assert!(parse_github_remote("not-a-url").is_none());
  }

  #[test]
  fn rejects_malformed_github_remote_paths() {
    for url in [
      "https://github.com/owner",
      "https://github.com/owner/",
      "https://github.com//repo.git",
      "https://github.com/owner/repo/extra",
      "git@github.com:owner/repo/extra.git",
    ] {
      assert!(parse_github_remote(url).is_none(), "accepted {url}");
    }
  }

  #[test]
  fn test_parse_trims_whitespace() {
    let info = parse_github_remote("  git@github.com:owner/repo.git  ").unwrap();
    assert_eq!(info.full_name, "owner/repo");
  }

  // ── get_git_remote_url_impl ──────────────────────────────────────────────

  fn write_git_config(dir: &TempDir, content: &str) {
    let git_dir = dir.path().join(".git");
    fs::create_dir_all(&git_dir).unwrap();
    fs::write(git_dir.join("config"), content).unwrap();
  }

  #[test]
  fn test_get_git_remote_url_parses_ssh_origin() {
    let dir = TempDir::new().unwrap();
    write_git_config(
      &dir,
      r#"[core]
    repositoryformatversion = 0
[remote "origin"]
    url = git@github.com:ziinc/treq.git
    fetch = +refs/heads/*:refs/remotes/origin/*
"#,
    );
    let info = get_git_remote_url_impl(dir.path().to_str().unwrap())
      .unwrap()
      .unwrap();
    assert_eq!(info.owner, "ziinc");
    assert_eq!(info.repo, "treq");
    assert_eq!(info.full_name, "ziinc/treq");
  }

  #[test]
  fn test_get_git_remote_url_parses_https_origin() {
    let dir = TempDir::new().unwrap();
    write_git_config(
      &dir,
      r#"[remote "origin"]
    url = https://github.com/ziinc/treq.git
"#,
    );
    let info = get_git_remote_url_impl(dir.path().to_str().unwrap())
      .unwrap()
      .unwrap();
    assert_eq!(info.full_name, "ziinc/treq");
  }

  #[test]
  fn test_get_git_remote_url_non_github_remote_returns_none() {
    let dir = TempDir::new().unwrap();
    write_git_config(
      &dir,
      r#"[remote "origin"]
    url = https://gitlab.com/owner/repo.git
"#,
    );
    let result = get_git_remote_url_impl(dir.path().to_str().unwrap()).unwrap();
    assert!(result.is_none());
  }

  #[test]
  fn test_get_git_remote_url_missing_git_dir_returns_none() {
    let dir = TempDir::new().unwrap();
    let result = get_git_remote_url_impl(dir.path().to_str().unwrap()).unwrap();
    assert!(result.is_none());
  }

  #[test]
  fn test_get_git_remote_url_ignores_non_origin_remotes() {
    let dir = TempDir::new().unwrap();
    write_git_config(
      &dir,
      r#"[remote "upstream"]
    url = git@github.com:owner/repo.git
"#,
    );
    let result = get_git_remote_url_impl(dir.path().to_str().unwrap()).unwrap();
    assert!(result.is_none());
  }

  #[test]
  fn reads_origin_from_git_worktree_common_config() {
    let dir = TempDir::new().unwrap();
    let common = dir.path().join("repo/.git");
    let worktree = dir.path().join("repo/.git/worktrees/feature");
    let checkout = dir.path().join("feature");
    fs::create_dir_all(&worktree).unwrap();
    fs::create_dir_all(&checkout).unwrap();
    fs::write(
      common.join("config"),
      "[remote \"origin\"]\n url = https://github.com/owner/repo.git\n",
    )
    .unwrap();
    fs::write(
      checkout.join(".git"),
      format!("gitdir: {}\n", worktree.display()),
    )
    .unwrap();

    let info = get_git_remote_url_impl(checkout.to_str().unwrap())
      .unwrap()
      .unwrap();
    assert_eq!(info.full_name, "owner/repo");
  }

  // ── get_pr_info_via_gh_impl ──────────────────────────────────────────────

  #[cfg(unix)]
  fn write_fake_gh(dir: &TempDir, script_body: &str) -> String {
    let path = dir.path().join("gh");
    let mut f = fs::File::create(&path).unwrap();
    writeln!(f, "#!/bin/sh").unwrap();
    write!(f, "{script_body}").unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path.to_str().unwrap().to_string()
  }

  #[test]
  #[cfg(unix)]
  fn test_get_pr_info_via_gh_returns_parsed_pr() {
    let bin_dir = TempDir::new().unwrap();
    let repo_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(
      &bin_dir,
      r#"echo '{"number":42,"title":"My PR","state":"OPEN","url":"https://github.com/o/r/pull/42","headRefName":"feat","baseRefName":"main","mergeStateStatus":"CLEAN","isDraft":false}'"#,
    );

    let info = get_pr_info_via_gh_impl(
      &gh_path,
      repo_dir.path().to_str().unwrap(),
      "feat",
      "/usr/bin:/bin",
    )
    .unwrap()
    .unwrap();

    assert_eq!(info.number, 42);
    assert_eq!(info.title, "My PR");
    assert_eq!(info.state, "OPEN");
    assert_eq!(info.head_ref_name, "feat");
    assert_eq!(info.base_ref_name, "main");
    assert_eq!(info.merge_state_status.as_deref(), Some("CLEAN"));
    assert!(!info.is_draft);
  }

  #[test]
  #[cfg(unix)]
  fn get_pr_info_parses_draft_flag() {
    let bin_dir = TempDir::new().unwrap();
    let repo_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(
      &bin_dir,
      r#"echo '{"number":7,"title":"Draft","state":"OPEN","url":"https://github.com/o/r/pull/7","headRefName":"feat","baseRefName":"main","mergeStateStatus":null,"isDraft":true}'"#,
    );

    let info = get_pr_info_via_gh_impl(
      &gh_path,
      repo_dir.path().to_str().unwrap(),
      "feat",
      "/usr/bin:/bin",
    )
    .unwrap()
    .unwrap();
    assert!(info.is_draft);
  }

  #[test]
  #[cfg(unix)]
  fn get_pr_info_parses_body() {
    let bin_dir = TempDir::new().unwrap();
    let repo_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(
      &bin_dir,
      r#"echo '{"number":7,"title":"T","state":"OPEN","url":"u","headRefName":"feat","baseRefName":"main","mergeStateStatus":null,"isDraft":false,"body":"**Why**: because"}'"#,
    );

    let info = get_pr_info_via_gh_impl(
      &gh_path,
      repo_dir.path().to_str().unwrap(),
      "feat",
      "/usr/bin:/bin",
    )
    .unwrap()
    .unwrap();
    assert_eq!(info.body, "**Why**: because");
  }

  #[test]
  #[cfg(unix)]
  fn get_pr_info_invokes_gh_with_exact_arguments_and_working_directory() {
    let bin_dir = TempDir::new().unwrap();
    let repo_dir = TempDir::new().unwrap();
    let expected_dir = repo_dir.path().canonicalize().unwrap();
    let expected_dir = expected_dir.display();
    let script = format!(
      r#"test "$PWD" = "{expected_dir}" || exit 8
test "$*" = "pr view --json number,title,state,url,headRefName,baseRefName,mergeStateStatus,isDraft,body -- feat" || exit 9
echo '{{"number":42,"title":"My PR","state":"OPEN","url":"https://github.com/o/r/pull/42","headRefName":"feat","baseRefName":"main","mergeStateStatus":null,"isDraft":false}}'"#,
    );
    let gh_path = write_fake_gh(&bin_dir, &script);

    let result = get_pr_info_via_gh_impl(
      &gh_path,
      repo_dir.path().to_str().unwrap(),
      "feat",
      "/usr/bin:/bin",
    );

    assert!(result.unwrap().is_some());
  }

  #[test]
  #[cfg(unix)]
  fn get_pr_info_passes_dash_prefixed_branch_as_positional() {
    let bin_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(
      &bin_dir,
      r#"test "$*" = "pr view --json number,title,state,url,headRefName,baseRefName,mergeStateStatus,isDraft,body -- --web" || exit 9
echo 'no pull requests found for branch "--web"' >&2
exit 1"#,
    );

    let result = get_pr_info_via_gh_impl(&gh_path, "/tmp", "--web", "/usr/bin:/bin").unwrap();

    assert!(result.is_none());
  }

  #[test]
  #[cfg(unix)]
  fn get_pr_checks_passes_dash_prefixed_branch_as_positional() {
    let bin_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(
      &bin_dir,
      r#"test "$*" = "pr checks --json name,bucket,link,startedAt,completedAt -- --web" || exit 9
echo 'no pull requests found for branch "--web"' >&2
exit 1"#,
    );

    let result = get_pr_checks_via_gh_impl(&gh_path, "/tmp", "--web", "/usr/bin:/bin").unwrap();

    assert!(result.is_none());
  }

  #[test]
  #[cfg(unix)]
  fn test_get_pr_info_via_gh_operational_error_is_returned() {
    let bin_dir = TempDir::new().unwrap();
    let repo_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(&bin_dir, "echo 'authentication required' >&2\nexit 1");

    let result = get_pr_info_via_gh_impl(
      &gh_path,
      repo_dir.path().to_str().unwrap(),
      "feat",
      "/usr/bin:/bin",
    );
    let Err(error) = result else {
      panic!("operational gh failure was treated as no PR")
    };

    assert!(error.contains("authentication required"));
  }

  #[test]
  #[cfg(unix)]
  fn test_get_pr_info_via_gh_no_pr_state_returns_none() {
    let bin_dir = TempDir::new().unwrap();
    let repo_dir = TempDir::new().unwrap();
    // gh exits 1 with a "no pull requests found" message on stderr
    let gh_path = write_fake_gh(&bin_dir, "echo 'no pull requests found' >&2\nexit 1");

    let result = get_pr_info_via_gh_impl(
      &gh_path,
      repo_dir.path().to_str().unwrap(),
      "unknown-branch",
      "/usr/bin:/bin",
    )
    .unwrap();

    assert!(result.is_none());
  }

  #[test]
  #[cfg(unix)]
  fn gh_create_pr_passes_draft_flag_when_requested() {
    let bin_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(
      &bin_dir,
      r#"test "$*" = "pr create --repo owner/repo --title T --body-file - --base main --head feat --draft" || exit 9
echo 'https://github.com/owner/repo/pull/7'"#,
    );

    let number = gh_create_pr_impl(
      &gh_path,
      "owner/repo",
      "T",
      "B",
      "main",
      "feat",
      true,
      "/usr/bin:/bin",
    )
    .unwrap();
    assert_eq!(number, 7);
  }

  #[test]
  #[cfg(unix)]
  fn gh_create_pr_omits_draft_flag_when_not_requested() {
    let bin_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(
      &bin_dir,
      r#"test "$*" = "pr create --repo owner/repo --title T --body-file - --base main --head feat" || exit 9
echo 'https://github.com/owner/repo/pull/8'"#,
    );

    let number = gh_create_pr_impl(
      &gh_path,
      "owner/repo",
      "T",
      "B",
      "main",
      "feat",
      false,
      "/usr/bin:/bin",
    )
    .unwrap();
    assert_eq!(number, 8);
  }

  /// Over Linux's 128 KiB MAX_ARG_STRLEN and starts with `--`: unsafe as argv.
  fn oversized_body() -> String {
    format!("--not-a-flag\n{}", "x".repeat(200_000))
  }

  #[test]
  #[cfg(unix)]
  fn gh_create_pr_sends_body_over_stdin() {
    let bin_dir = TempDir::new().unwrap();
    let body_path = bin_dir.path().join("body");
    let gh_path = write_fake_gh(
      &bin_dir,
      &format!(
        "cat > '{}'\necho 'https://github.com/owner/repo/pull/9'",
        body_path.display()
      ),
    );
    let body = oversized_body();

    let number = gh_create_pr_impl(
      &gh_path,
      "owner/repo",
      "T",
      &body,
      "main",
      "feat",
      false,
      "/usr/bin:/bin",
    )
    .unwrap();

    assert_eq!(number, 9);
    assert_eq!(fs::read_to_string(&body_path).unwrap(), body);
  }

  #[test]
  #[cfg(unix)]
  fn gh_create_pr_comment_sends_body_over_stdin() {
    let bin_dir = TempDir::new().unwrap();
    let body_path = bin_dir.path().join("body");
    let gh_path = write_fake_gh(
      &bin_dir,
      &format!(
        "test \"$*\" = \"pr comment 5 --repo owner/repo --body-file -\" || exit 9\ncat > '{}'",
        body_path.display()
      ),
    );
    let body = oversized_body();

    gh_create_pr_comment_impl(&gh_path, "owner/repo", 5, &body, "/usr/bin:/bin").unwrap();

    assert_eq!(fs::read_to_string(&body_path).unwrap(), body);
  }

  #[test]
  #[cfg(unix)]
  fn gh_create_issue_sends_body_over_stdin() {
    let bin_dir = TempDir::new().unwrap();
    let body_path = bin_dir.path().join("body");
    let gh_path = write_fake_gh(
      &bin_dir,
      &format!(
        "test \"$*\" = \"issue create --repo owner/repo --title T --body-file -\" || exit 9\ncat > '{}'\necho 'https://github.com/owner/repo/issues/3'",
        body_path.display()
      ),
    );
    let body = oversized_body();

    let number = gh_create_issue_impl(&gh_path, "owner/repo", "T", &body, "/usr/bin:/bin").unwrap();

    assert_eq!(number, 3);
    assert_eq!(fs::read_to_string(&body_path).unwrap(), body);
  }

  #[test]
  #[cfg(unix)]
  fn gh_create_issue_skips_numeric_lines_that_are_not_the_issue_url() {
    let bin_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(
      &bin_dir,
      "cat > /dev/null\necho 'Using template .github/ISSUE_TEMPLATE/2'\necho 'https://github.com/owner/repo/issues/8'",
    );

    let number = gh_create_issue_impl(&gh_path, "owner/repo", "T", "", "/usr/bin:/bin").unwrap();

    assert_eq!(number, 8);
  }

  #[test]
  #[cfg(unix)]
  fn gh_create_issue_errors_when_output_has_no_issue_url() {
    let bin_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(
      &bin_dir,
      "cat > /dev/null\necho 'https://github.com/owner/repo/pull/8'",
    );

    let err = gh_create_issue_impl(&gh_path, "owner/repo", "T", "", "/usr/bin:/bin").unwrap_err();

    assert!(err.contains("Could not parse issue number"), "{err}");
  }

  #[test]
  #[cfg(unix)]
  fn gh_create_pr_skips_numeric_lines_that_are_not_the_pr_url() {
    let bin_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(
      &bin_dir,
      "cat > /dev/null\necho 'Pushed branch feat/2'\necho 'https://github.com/owner/repo/pull/5'",
    );

    let number = gh_create_pr_impl(
      &gh_path,
      "owner/repo",
      "T",
      "",
      "main",
      "feat/2",
      false,
      "/usr/bin:/bin",
    )
    .unwrap();

    assert_eq!(number, 5);
  }

  #[test]
  #[cfg(unix)]
  fn gh_close_issue_passes_the_close_reason() {
    let bin_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(
      &bin_dir,
      r#"test "$*" = "issue close 4 --repo owner/repo --reason not planned" || exit 9"#,
    );

    gh_close_issue_impl(
      &gh_path,
      "owner/repo",
      4,
      Some("not planned"),
      "/usr/bin:/bin",
    )
    .unwrap();
  }

  #[test]
  #[cfg(unix)]
  fn gh_close_issue_omits_reason_when_none_given() {
    let bin_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(
      &bin_dir,
      r#"test "$*" = "issue close 4 --repo owner/repo" || exit 9"#,
    );

    gh_close_issue_impl(&gh_path, "owner/repo", 4, None, "/usr/bin:/bin").unwrap();
  }

  #[test]
  #[cfg(unix)]
  fn gh_close_issue_rejects_unknown_reason_without_running_gh() {
    let bin_dir = TempDir::new().unwrap();
    let marker = bin_dir.path().join("ran");
    let gh_path = write_fake_gh(&bin_dir, &format!("touch '{}'", marker.display()));

    let err = gh_close_issue_impl(
      &gh_path,
      "owner/repo",
      4,
      Some("duplicate"),
      "/usr/bin:/bin",
    )
    .unwrap_err();

    assert!(err.contains("duplicate"), "{err}");
    assert!(!marker.exists());
  }

  #[test]
  #[cfg(unix)]
  fn gh_edit_issue_sends_title_as_arg_and_body_over_stdin() {
    let bin_dir = TempDir::new().unwrap();
    let body_path = bin_dir.path().join("body");
    let gh_path = write_fake_gh(
      &bin_dir,
      &format!(
        "test \"$*\" = \"issue edit 6 --repo owner/repo --title New title --body-file -\" || exit 9\ncat > '{}'",
        body_path.display()
      ),
    );
    let body = oversized_body();

    gh_edit_issue_impl(
      &gh_path,
      "owner/repo",
      6,
      "New title",
      &body,
      "/usr/bin:/bin",
    )
    .unwrap();

    assert_eq!(fs::read_to_string(&body_path).unwrap(), body);
  }

  #[test]
  #[cfg(unix)]
  fn gh_edit_pr_sends_title_as_arg_and_body_over_stdin() {
    let bin_dir = TempDir::new().unwrap();
    let body_path = bin_dir.path().join("body");
    let gh_path = write_fake_gh(
      &bin_dir,
      &format!(
        "test \"$*\" = \"pr edit 7 --repo owner/repo --title New title --body-file -\" || exit 9\ncat > '{}'",
        body_path.display()
      ),
    );
    let body = oversized_body();

    gh_edit_pr_impl(
      &gh_path,
      "owner/repo",
      7,
      Some("New title"),
      Some(&body),
      "/usr/bin:/bin",
    )
    .unwrap();

    assert_eq!(fs::read_to_string(&body_path).unwrap(), body);
  }

  #[test]
  #[cfg(unix)]
  fn gh_edit_pr_leaves_out_fields_it_does_not_change() {
    let bin_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(
      &bin_dir,
      "test \"$*\" = \"pr edit 7 --repo owner/repo --title T\" || exit 9\ncat > /dev/null",
    );

    gh_edit_pr_impl(&gh_path, "owner/repo", 7, Some("T"), None, "/usr/bin:/bin").unwrap();
  }

  #[test]
  #[cfg(unix)]
  fn gh_edit_issue_reports_gh_error() {
    let bin_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(
      &bin_dir,
      "cat > /dev/null\necho 'GraphQL: Could not resolve to an issue' >&2\nexit 1",
    );

    let err = gh_edit_issue_impl(&gh_path, "owner/repo", 6, "T", "", "/usr/bin:/bin").unwrap_err();

    assert!(err.contains("Could not resolve to an issue"), "{err}");
  }

  #[test]
  #[cfg(unix)]
  fn gh_delete_issue_confirms_non_interactively() {
    let bin_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(
      &bin_dir,
      r#"test "$*" = "issue delete 6 --repo owner/repo --yes" || exit 9"#,
    );

    gh_delete_issue_impl(&gh_path, "owner/repo", 6, "/usr/bin:/bin").unwrap();
  }

  const ISSUES_GRAPHQL_PAGE: &str = r#"{"data":{"repository":{"issues":{"nodes":[{"number":9,"title":"Newest","state":"OPEN","url":"https://github.com/owner/repo/issues/9","createdAt":"2026-01-02T00:00:00Z","updatedAt":"2026-01-03T00:00:00Z","author":{"login":"alice"},"labels":{"nodes":[{"name":"bug","color":"d73a4a"}]}},{"number":8,"title":"From a deleted account","state":"CLOSED","url":"https://github.com/owner/repo/issues/8","createdAt":"2026-01-01T00:00:00Z","updatedAt":"2026-01-01T00:00:00Z","author":null,"labels":{"nodes":[]}}],"pageInfo":{"hasNextPage":true,"endCursor":"Y3Vyc29yOjg="}}}}}"#;

  /// Fake gh that records each argument (one per `\0`-terminated record) and
  /// prints `stdout`.
  fn write_recording_gh(dir: &TempDir, stdout: &str) -> (String, std::path::PathBuf) {
    let args_path = dir.path().join("args");
    let out_path = dir.path().join("out.json");
    fs::write(&out_path, stdout).unwrap();
    let gh = write_fake_gh(
      dir,
      &format!(
        "for a in \"$@\"; do printf '%s\\0' \"$a\"; done > '{}'\ncat '{}'",
        args_path.display(),
        out_path.display()
      ),
    );
    (gh, args_path)
  }

  fn recorded_args(path: &std::path::Path) -> Vec<String> {
    fs::read_to_string(path)
      .unwrap()
      .split('\0')
      .filter(|a| !a.is_empty())
      .map(str::to_string)
      .collect()
  }

  #[test]
  #[cfg(unix)]
  fn gh_list_issues_reads_one_graphql_page_with_its_cursor() {
    let bin_dir = TempDir::new().unwrap();
    let (gh_path, args_path) = write_recording_gh(&bin_dir, ISSUES_GRAPHQL_PAGE);

    let page =
      gh_list_issues_impl(&gh_path, "owner/repo", "open", 30, None, "/usr/bin:/bin").unwrap();

    let args = recorded_args(&args_path);
    assert_eq!(&args[..2], ["api", "graphql"]);
    assert!(args.contains(&"owner=owner".to_string()), "{args:?}");
    assert!(args.contains(&"name=repo".to_string()), "{args:?}");
    assert!(args.contains(&"first=30".to_string()), "{args:?}");
    assert!(!args.iter().any(|a| a.starts_with("after=")), "{args:?}");
    let query = args.iter().find(|a| a.starts_with("query=")).unwrap();
    assert!(query.contains("states: [OPEN]"), "{query}");

    assert!(page.has_more);
    assert_eq!(page.end_cursor.as_deref(), Some("Y3Vyc29yOjg="));
    let numbers: Vec<u64> = page.items.iter().map(|i| i.number).collect();
    assert_eq!(numbers, [9, 8]);
    let newest = &page.items[0];
    assert_eq!(newest.title, "Newest");
    assert_eq!(newest.state, "OPEN");
    assert_eq!(newest.author.login, "alice");
    assert_eq!(newest.labels[0].name, "bug");
    assert_eq!(newest.created_at, "2026-01-02T00:00:00Z");
  }

  #[test]
  #[cfg(unix)]
  fn gh_list_issues_passes_the_after_cursor() {
    let bin_dir = TempDir::new().unwrap();
    let (gh_path, args_path) = write_recording_gh(&bin_dir, ISSUES_GRAPHQL_PAGE);

    gh_list_issues_impl(
      &gh_path,
      "owner/repo",
      "closed",
      30,
      Some("Y3Vyc29yOjg="),
      "/usr/bin:/bin",
    )
    .unwrap();

    let args = recorded_args(&args_path);
    assert!(args.contains(&"after=Y3Vyc29yOjg=".to_string()), "{args:?}");
    let query = args.iter().find(|a| a.starts_with("query=")).unwrap();
    assert!(query.contains("states: [CLOSED]"), "{query}");
  }

  #[test]
  #[cfg(unix)]
  fn gh_list_issues_all_state_applies_no_state_filter() {
    let bin_dir = TempDir::new().unwrap();
    let (gh_path, args_path) = write_recording_gh(&bin_dir, ISSUES_GRAPHQL_PAGE);

    gh_list_issues_impl(&gh_path, "owner/repo", "all", 30, None, "/usr/bin:/bin").unwrap();

    let args = recorded_args(&args_path);
    let query = args.iter().find(|a| a.starts_with("query=")).unwrap();
    assert!(!query.contains("states:"), "{query}");
  }

  #[test]
  #[cfg(unix)]
  fn gh_list_issues_shows_a_deleted_author_as_ghost() {
    let bin_dir = TempDir::new().unwrap();
    let (gh_path, _) = write_recording_gh(&bin_dir, ISSUES_GRAPHQL_PAGE);

    let page =
      gh_list_issues_impl(&gh_path, "owner/repo", "open", 30, None, "/usr/bin:/bin").unwrap();

    assert_eq!(page.items[1].author.login, "ghost");
  }

  #[test]
  #[cfg(unix)]
  fn gh_list_issues_rejects_bad_input_without_running_gh() {
    let bin_dir = TempDir::new().unwrap();
    let marker = bin_dir.path().join("ran");
    let gh_path = write_fake_gh(&bin_dir, &format!("touch '{}'", marker.display()));

    let bad_repo = gh_list_issues_impl(&gh_path, "no-slash", "open", 30, None, "/usr/bin:/bin");
    let bad_state =
      gh_list_issues_impl(&gh_path, "owner/repo", "merged", 30, None, "/usr/bin:/bin");

    assert!(bad_repo.err().unwrap().contains("no-slash"));
    assert!(bad_state.err().unwrap().contains("merged"));
    assert!(!marker.exists());
  }

  #[test]
  #[cfg(unix)]
  fn gh_create_issue_comment_sends_body_over_stdin() {
    let bin_dir = TempDir::new().unwrap();
    let body_path = bin_dir.path().join("body");
    let gh_path = write_fake_gh(
      &bin_dir,
      &format!(
        "test \"$*\" = \"issue comment 4 --repo owner/repo --body-file -\" || exit 9\ncat > '{}'",
        body_path.display()
      ),
    );
    let body = oversized_body();

    gh_create_issue_comment_impl(&gh_path, "owner/repo", 4, &body, "/usr/bin:/bin").unwrap();

    assert_eq!(fs::read_to_string(&body_path).unwrap(), body);
  }

  #[test]
  #[cfg(unix)]
  fn gh_create_pr_reports_gh_error_when_gh_exits_without_reading_body() {
    let bin_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(&bin_dir, "echo 'no permission' >&2\nexit 1");

    let err = gh_create_pr_impl(
      &gh_path,
      "owner/repo",
      "T",
      &oversized_body(),
      "main",
      "feat",
      false,
      "/usr/bin:/bin",
    )
    .unwrap_err();

    assert!(err.contains("no permission"), "{err}");
  }

  #[test]
  #[cfg(unix)]
  fn gh_set_pr_draft_marks_ready_without_undo() {
    let bin_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(
      &bin_dir,
      r#"test "$*" = "pr ready 9 --repo owner/repo" || exit 9
echo ok"#,
    );
    gh_set_pr_draft_impl(&gh_path, "owner/repo", 9, false, "/usr/bin:/bin").unwrap();
  }

  #[test]
  #[cfg(unix)]
  fn gh_set_pr_draft_converts_to_draft_with_undo() {
    let bin_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(
      &bin_dir,
      r#"test "$*" = "pr ready 9 --repo owner/repo --undo" || exit 9
echo ok"#,
    );
    gh_set_pr_draft_impl(&gh_path, "owner/repo", 9, true, "/usr/bin:/bin").unwrap();
  }

  #[test]
  #[cfg(unix)]
  fn gh_list_prs_passes_draft_flag_when_state_is_draft() {
    let bin_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(
      &bin_dir,
      r#"test "$*" = "pr list --repo owner/repo --state open --draft --json number,title,state,url,author,labels,headRefName,baseRefName,mergeStateStatus,createdAt,updatedAt,isDraft --limit 30" || exit 9
echo '[]'"#,
    );
    let page = gh_list_prs_impl(&gh_path, "owner/repo", "draft", 30, 1, "/usr/bin:/bin").unwrap();
    assert!(page.items.is_empty());
    assert!(!page.has_more);
  }

  #[test]
  #[cfg(unix)]
  fn gh_list_prs_includes_merge_state_status() {
    let bin_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(
      &bin_dir,
      r#"case "$*" in *mergeStateStatus*) ;; *) exit 9 ;; esac
echo '[{"number":3,"title":"T","state":"OPEN","url":"u","author":{"login":"a"},"labels":[],"headRefName":"feat","baseRefName":"main","mergeStateStatus":"DIRTY","createdAt":"c","updatedAt":"u","isDraft":false}]'"#,
    );
    let page = gh_list_prs_impl(&gh_path, "owner/repo", "open", 30, 1, "/usr/bin:/bin").unwrap();
    assert_eq!(page.items[0].merge_state_status.as_deref(), Some("DIRTY"));
  }

  #[test]
  #[cfg(unix)]
  fn gh_list_prs_excludes_drafts_when_state_is_open() {
    let bin_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(
      &bin_dir,
      r#"test "$*" = "pr list --repo owner/repo --state open --search draft:false --json number,title,state,url,author,labels,headRefName,baseRefName,mergeStateStatus,createdAt,updatedAt,isDraft --limit 30" || exit 9
echo '[]'"#,
    );
    let page = gh_list_prs_impl(&gh_path, "owner/repo", "open", 30, 1, "/usr/bin:/bin").unwrap();
    assert!(page.items.is_empty());
  }

  #[test]
  #[cfg(unix)]
  fn gh_search_open_prs_lists_own_prs_newest_first() {
    let bin_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(
      &bin_dir,
      r#"test "$*" = "pr list --repo owner/repo --state open --search sort:created-desc --author @me --json number,title,state,url,author,labels,headRefName,baseRefName,mergeStateStatus,createdAt,updatedAt,isDraft --limit 50" || exit 9
echo '[{"number":3,"title":"T","state":"OPEN","url":"u","author":{"login":"a"},"labels":[],"headRefName":"feat","baseRefName":"main","createdAt":"c","updatedAt":"u","isDraft":true}]'"#,
    );
    let prs =
      gh_search_open_prs_impl(&gh_path, "owner/repo", true, "", 50, "/usr/bin:/bin").unwrap();
    assert_eq!(prs[0].head_ref_name, "feat");
    assert!(prs[0].is_draft);
  }

  #[test]
  #[cfg(unix)]
  fn gh_search_open_prs_searches_everyones_prs() {
    let bin_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(
      &bin_dir,
      r#"test "$4 $6 $7 $8" = "owner/repo open --search login fix sort:created-desc" || exit 9
case "$*" in *--author*) exit 9 ;; esac
echo '[]'"#,
    );
    let prs = gh_search_open_prs_impl(
      &gh_path,
      "owner/repo",
      false,
      " login fix ",
      50,
      "/usr/bin:/bin",
    )
    .unwrap();
    assert!(prs.is_empty());
  }

  // ── check duration ───────────────────────────────────────────────────────

  #[test]
  fn check_duration_secs_from_completed_run() {
    let secs = check_duration_secs(Some("2026-08-08T22:00:00Z"), Some("2026-08-08T22:05:56Z"));
    assert_eq!(secs, Some(5 * 60 + 56));
  }

  #[test]
  fn check_duration_secs_is_none_while_still_running() {
    assert_eq!(
      check_duration_secs(Some("2026-08-08T22:00:00Z"), None),
      None
    );
    // `gh` reports an unfinished run's completedAt as Go's zero time.
    assert_eq!(
      check_duration_secs(Some("2026-08-08T22:00:00Z"), Some("0001-01-01T00:00:00Z")),
      None
    );
    assert_eq!(
      check_duration_secs(Some("0001-01-01T00:00:00Z"), Some("0001-01-01T00:00:00Z")),
      None
    );
  }

  #[test]
  fn check_duration_secs_clamps_inverted_timestamps() {
    // Skipped checks sometimes report startedAt after completedAt.
    let secs = check_duration_secs(Some("2026-08-08T22:02:45Z"), Some("2026-08-08T22:02:35Z"));
    assert_eq!(secs, Some(0));
  }

  #[test]
  #[cfg(unix)]
  fn get_pr_checks_includes_duration_from_timestamps() {
    let bin_dir = TempDir::new().unwrap();
    let repo_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(
      &bin_dir,
      r#"echo '[{"name":"build","bucket":"pass","link":"https://x/1","startedAt":"2026-08-08T22:00:00Z","completedAt":"2026-08-08T22:05:56Z"}]'"#,
    );

    let status = get_pr_checks_via_gh_impl(
      &gh_path,
      repo_dir.path().to_str().unwrap(),
      "feat",
      "/usr/bin:/bin",
    )
    .unwrap()
    .unwrap();

    assert_eq!(status.checks[0].duration_secs, Some(5 * 60 + 56));
  }

  // ── get_pr_checks_via_gh_impl ────────────────────────────────────────────

  #[test]
  #[cfg(unix)]
  fn test_get_pr_checks_all_passing_reports_success() {
    let bin_dir = TempDir::new().unwrap();
    let repo_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(
      &bin_dir,
      r#"echo '[{"name":"build","bucket":"pass","link":"https://x/1"},{"name":"lint","bucket":"skipping","link":"https://x/2"}]'"#,
    );

    let status = get_pr_checks_via_gh_impl(
      &gh_path,
      repo_dir.path().to_str().unwrap(),
      "feat",
      "/usr/bin:/bin",
    )
    .unwrap()
    .unwrap();

    assert_eq!(status.state, "success");
    assert_eq!(status.total, 2);
    assert_eq!(status.passed, 2);
    assert_eq!(status.failed, 0);
    assert_eq!(status.pending, 0);
  }

  #[test]
  #[cfg(unix)]
  fn test_get_pr_checks_any_failure_reports_failure() {
    let bin_dir = TempDir::new().unwrap();
    let repo_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(
      &bin_dir,
      r#"echo '[{"name":"build","bucket":"pass","link":"https://x/1"},{"name":"test","bucket":"fail","link":"https://x/2"},{"name":"lint","bucket":"pending","link":"https://x/3"}]'"#,
    );

    let status = get_pr_checks_via_gh_impl(
      &gh_path,
      repo_dir.path().to_str().unwrap(),
      "feat",
      "/usr/bin:/bin",
    )
    .unwrap()
    .unwrap();

    assert_eq!(status.state, "failure");
    assert_eq!(status.total, 3);
    assert_eq!(status.passed, 1);
    assert_eq!(status.failed, 1);
    assert_eq!(status.pending, 1);
  }

  #[test]
  #[cfg(unix)]
  fn test_get_pr_checks_pending_without_failure_reports_pending() {
    let bin_dir = TempDir::new().unwrap();
    let repo_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(
      &bin_dir,
      r#"echo '[{"name":"build","bucket":"pass","link":"https://x/1"},{"name":"test","bucket":"pending","link":"https://x/2"}]'"#,
    );

    let status = get_pr_checks_via_gh_impl(
      &gh_path,
      repo_dir.path().to_str().unwrap(),
      "feat",
      "/usr/bin:/bin",
    )
    .unwrap()
    .unwrap();

    assert_eq!(status.state, "pending");
  }

  #[test]
  #[cfg(unix)]
  fn test_get_pr_checks_no_checks_reported_returns_none() {
    let bin_dir = TempDir::new().unwrap();
    let repo_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(
      &bin_dir,
      "echo 'no checks reported on the feat branch' >&2\nexit 1",
    );

    let result = get_pr_checks_via_gh_impl(
      &gh_path,
      repo_dir.path().to_str().unwrap(),
      "feat",
      "/usr/bin:/bin",
    )
    .unwrap();

    assert!(result.is_none());
  }

  #[test]
  #[cfg(unix)]
  fn test_get_pr_checks_no_pr_returns_none() {
    let bin_dir = TempDir::new().unwrap();
    let repo_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(&bin_dir, "echo 'no pull requests found' >&2\nexit 1");

    let result = get_pr_checks_via_gh_impl(
      &gh_path,
      repo_dir.path().to_str().unwrap(),
      "unknown-branch",
      "/usr/bin:/bin",
    )
    .unwrap();

    assert!(result.is_none());
  }

  #[test]
  #[cfg(unix)]
  fn test_get_pr_checks_operational_error_is_returned() {
    let bin_dir = TempDir::new().unwrap();
    let repo_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(&bin_dir, "echo 'authentication required' >&2\nexit 1");

    let result = get_pr_checks_via_gh_impl(
      &gh_path,
      repo_dir.path().to_str().unwrap(),
      "feat",
      "/usr/bin:/bin",
    );
    let Err(error) = result else {
      panic!("operational gh failure was treated as no checks")
    };
    assert!(error.contains("authentication required"));
  }

  #[test]
  #[cfg(unix)]
  fn get_pr_checks_invokes_gh_with_exact_arguments_and_working_directory() {
    let bin_dir = TempDir::new().unwrap();
    let repo_dir = TempDir::new().unwrap();
    let expected_dir = repo_dir.path().canonicalize().unwrap();
    let expected_dir = expected_dir.display();
    let script = format!(
      r#"test "$PWD" = "{expected_dir}" || exit 8
test "$*" = "pr checks --json name,bucket,link,startedAt,completedAt -- feat" || exit 9
echo '[{{"name":"build","bucket":"pass","link":"https://x/1"}}]'"#,
    );
    let gh_path = write_fake_gh(&bin_dir, &script);

    let result = get_pr_checks_via_gh_impl(
      &gh_path,
      repo_dir.path().to_str().unwrap(),
      "feat",
      "/usr/bin:/bin",
    );

    assert!(result.unwrap().is_some());
  }

  // ── get_pr_checks_for_pr_impl ────────────────────────────────────────────

  #[test]
  #[cfg(unix)]
  fn get_pr_checks_for_pr_rolls_up_like_the_branch_based_lookup() {
    let bin_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(
      &bin_dir,
      r#"echo '[{"name":"build","bucket":"pass","link":"https://x/1"},{"name":"test","bucket":"fail","link":"https://x/2"}]'"#,
    );

    let status = get_pr_checks_for_pr_impl(&gh_path, "owner/repo", 42, "/usr/bin:/bin")
      .unwrap()
      .unwrap();

    assert_eq!(status.state, "failure");
    assert_eq!(status.total, 2);
    assert_eq!(status.passed, 1);
    assert_eq!(status.failed, 1);
  }

  #[test]
  #[cfg(unix)]
  fn get_pr_checks_for_pr_invokes_gh_with_exact_arguments() {
    let bin_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(
      &bin_dir,
      r#"test "$*" = "pr checks 42 --repo owner/repo --json name,bucket,link,startedAt,completedAt" || exit 9
echo '[{"name":"build","bucket":"pass","link":"https://x/1"}]'"#,
    );

    let result = get_pr_checks_for_pr_impl(&gh_path, "owner/repo", 42, "/usr/bin:/bin");

    assert!(result.unwrap().is_some());
  }

  #[test]
  #[cfg(unix)]
  fn get_pr_checks_for_pr_no_checks_reported_returns_none() {
    let bin_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(
      &bin_dir,
      "echo 'no checks reported on the feat branch' >&2\nexit 1",
    );

    let result = get_pr_checks_for_pr_impl(&gh_path, "owner/repo", 42, "/usr/bin:/bin").unwrap();

    assert!(result.is_none());
  }

  #[test]
  #[cfg(unix)]
  fn get_pr_checks_for_pr_operational_error_is_returned() {
    let bin_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(&bin_dir, "echo 'authentication required' >&2\nexit 1");

    let result = get_pr_checks_for_pr_impl(&gh_path, "owner/repo", 42, "/usr/bin:/bin");
    let Err(error) = result else {
      panic!("operational gh failure was treated as no checks")
    };
    assert!(error.contains("authentication required"));
  }

  #[test]
  fn test_get_pr_info_via_gh_bad_binary_returns_err() {
    let result = get_pr_info_via_gh_impl("/nonexistent/gh", "/tmp", "feat", "/usr/bin:/bin");
    assert!(result.is_err());
  }

  #[test]
  fn take_list_page_returns_first_page_and_has_more_when_full() {
    // Page 1 is fetched with --limit (limit * page) == 2
    let page = take_list_page(vec![1, 2], 2, 1);
    assert_eq!(page.items, vec![1, 2]);
    assert!(page.has_more);
  }

  #[test]
  fn take_list_page_returns_second_page_slice() {
    // Page 2 is fetched with --limit 4
    let page = take_list_page(vec![1, 2, 3, 4], 2, 2);
    assert_eq!(page.items, vec![3, 4]);
    assert!(page.has_more);
  }

  #[test]
  fn take_list_page_has_more_false_when_short_of_full_prefix() {
    let page = take_list_page(vec![1, 2, 3], 2, 2);
    assert_eq!(page.items, vec![3]);
    assert!(!page.has_more);
  }

  #[test]
  fn take_list_page_has_more_false_on_short_first_page() {
    let page = take_list_page(vec![1], 2, 1);
    assert_eq!(page.items, vec![1]);
    assert!(!page.has_more);
  }

  // ── gh_list_pr_review_threads_impl ───────────────────────────────────────

  #[test]
  #[cfg(unix)]
  fn test_gh_list_pr_review_threads_parses_and_maps_fields() {
    let bin_dir = TempDir::new().unwrap();
    let response = serde_json::json!({
        "data": {
            "repository": {
                "pullRequest": {
                    "reviewThreads": {
                        "pageInfo": {"hasNextPage": false, "endCursor": null},
                        "nodes": [{
                            "id": "PRRT_1",
                            "isResolved": true,
                            "isOutdated": false,
                            "path": "src/main.rs",
                            "line": 42,
                            "diffSide": "RIGHT",
                            "comments": {
                                "nodes": [{
                                    "id": "PRRC_1",
                                    "body": "nit: rename this",
                                    "createdAt": "2026-01-01T00:00:00Z",
                                    "url": "https://github.com/o/r/pull/1#discussion_r1",
                                    "diffHunk": "@@ -1,3 +1,3 @@\n-a\n+b",
                                    "author": {"login": "reviewer1", "avatarUrl": "https://avatars.example/reviewer1.png"}
                                }]
                            }
                        }]
                    }
                }
            }
        }
    });
    let script = format!("cat <<'EOF'\n{response}\nEOF\n");
    let gh_path = write_fake_gh(&bin_dir, &script);

    let threads =
      gh_list_pr_review_threads_impl(&gh_path, "owner", "repo", 1, "/usr/bin:/bin").unwrap();

    assert_eq!(threads.len(), 1);
    let thread = &threads[0];
    assert_eq!(thread.id, "PRRT_1");
    assert!(thread.is_resolved);
    assert!(!thread.is_outdated);
    assert_eq!(thread.path, "src/main.rs");
    assert_eq!(thread.line, Some(42));
    assert_eq!(thread.diff_side, "RIGHT");
    assert_eq!(thread.comments.len(), 1);
    assert_eq!(thread.comments[0].body, "nit: rename this");
    assert_eq!(thread.comments[0].author.login, "reviewer1");
    assert_eq!(
      thread.comments[0].author.avatar_url.as_deref(),
      Some("https://avatars.example/reviewer1.png")
    );
  }

  #[test]
  #[cfg(unix)]
  fn test_gh_list_pr_review_threads_defaults_deleted_author_to_ghost() {
    let bin_dir = TempDir::new().unwrap();
    let response = serde_json::json!({
        "data": {"repository": {"pullRequest": {"reviewThreads": {
            "pageInfo": {"hasNextPage": false, "endCursor": null},
            "nodes": [{
                "id": "PRRT_1", "isResolved": false, "isOutdated": false,
                "path": "a.rs", "line": 1, "diffSide": "RIGHT",
                "comments": {"nodes": [{
                    "id": "PRRC_1", "body": "hi", "createdAt": "2026-01-01T00:00:00Z",
                    "url": "https://github.com/o/r/pull/1#discussion_r1",
                    "diffHunk": "@@ -1 +1 @@", "author": null
                }]}
            }]
        }}}}
    });
    let script = format!("cat <<'EOF'\n{response}\nEOF\n");
    let gh_path = write_fake_gh(&bin_dir, &script);

    let threads =
      gh_list_pr_review_threads_impl(&gh_path, "owner", "repo", 1, "/usr/bin:/bin").unwrap();

    assert_eq!(threads[0].comments[0].author.login, "ghost");
    assert_eq!(threads[0].comments[0].author.avatar_url, None);
  }

  #[test]
  #[cfg(unix)]
  fn test_gh_list_pr_review_threads_paginates_across_pages() {
    let bin_dir = TempDir::new().unwrap();
    let page1 = serde_json::json!({
        "data": {"repository": {"pullRequest": {"reviewThreads": {
            "pageInfo": {"hasNextPage": true, "endCursor": "CURSOR1"},
            "nodes": [{
                "id": "T1", "isResolved": false, "isOutdated": false,
                "path": "a.rs", "line": 1, "diffSide": "RIGHT",
                "comments": {"nodes": []}
            }]
        }}}}
    });
    let page2 = serde_json::json!({
        "data": {"repository": {"pullRequest": {"reviewThreads": {
            "pageInfo": {"hasNextPage": false, "endCursor": null},
            "nodes": [{
                "id": "T2", "isResolved": false, "isOutdated": false,
                "path": "b.rs", "line": 2, "diffSide": "RIGHT",
                "comments": {"nodes": []}
            }]
        }}}}
    });
    let script = format!(
      r#"case "$*" in
  *"after=CURSOR1"*) cat <<'EOF'
{page2}
EOF
  ;;
  *) cat <<'EOF'
{page1}
EOF
  ;;
esac
"#,
    );
    let gh_path = write_fake_gh(&bin_dir, &script);

    let threads =
      gh_list_pr_review_threads_impl(&gh_path, "owner", "repo", 1, "/usr/bin:/bin").unwrap();

    assert_eq!(threads.len(), 2);
    assert_eq!(threads[0].id, "T1");
    assert_eq!(threads[1].id, "T2");
  }

  #[test]
  #[cfg(unix)]
  fn test_gh_list_pr_review_threads_returns_empty_when_no_pull_request() {
    let bin_dir = TempDir::new().unwrap();
    let script = "cat <<'EOF'\n{\"data\":{\"repository\":{\"pullRequest\":null}}}\nEOF\n";
    let gh_path = write_fake_gh(&bin_dir, script);

    let threads =
      gh_list_pr_review_threads_impl(&gh_path, "owner", "repo", 1, "/usr/bin:/bin").unwrap();

    assert!(threads.is_empty());
  }

  #[test]
  #[cfg(unix)]
  fn test_gh_list_pr_review_threads_propagates_gh_error() {
    let bin_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(&bin_dir, "echo 'GraphQL error' >&2\nexit 1");

    let result = gh_list_pr_review_threads_impl(&gh_path, "owner", "repo", 1, "/usr/bin:/bin");

    assert!(result.is_err());
  }

  // ── PR review (diff, thread reply, submit) ───────────────────────────────

  /// Fake gh that records argv to `args` and stdin to `stdin` in `dir`, then
  /// prints `stdout`.
  #[cfg(unix)]
  fn write_stdin_recording_gh(bin_dir: &TempDir, stdout: &str) -> String {
    let dir = bin_dir.path().display();
    write_fake_gh(
      bin_dir,
      &format!(
        "printf '%s\\n' \"$@\" > {dir}/args\ncat > {dir}/stdin\ncat <<'TREQ_EOF'\n{stdout}\nTREQ_EOF\n"
      ),
    )
  }

  #[test]
  #[cfg(unix)]
  fn test_gh_pr_diff_returns_unified_diff() {
    let bin_dir = TempDir::new().unwrap();
    let gh_path = write_stdin_recording_gh(&bin_dir, "diff --git a/x b/x\n+hi");

    let diff = gh_pr_diff_impl(&gh_path, "o/r", 7, "/usr/bin:/bin").unwrap();

    assert!(diff.starts_with("diff --git a/x b/x\n+hi"));
    let args = fs::read_to_string(bin_dir.path().join("args")).unwrap();
    assert_eq!(args, "pr\ndiff\n7\n--repo\no/r\n");
  }

  #[test]
  #[cfg(unix)]
  fn test_gh_pr_diff_propagates_error() {
    let bin_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(&bin_dir, "echo nope >&2\nexit 1");
    assert!(gh_pr_diff_impl(&gh_path, "o/r", 7, "/usr/bin:/bin").is_err());
  }

  #[test]
  #[cfg(unix)]
  fn test_gh_reply_pr_review_thread_sends_graphql_mutation_on_stdin() {
    let bin_dir = TempDir::new().unwrap();
    let gh_path = write_stdin_recording_gh(&bin_dir, r#"{"data":{}}"#);

    gh_reply_pr_review_thread_impl(&gh_path, "PRRT_1", "thanks -- fixed", "/usr/bin:/bin").unwrap();

    let args = fs::read_to_string(bin_dir.path().join("args")).unwrap();
    assert_eq!(args, "api\ngraphql\n--input\n-\n");
    let stdin: serde_json::Value =
      serde_json::from_str(&fs::read_to_string(bin_dir.path().join("stdin")).unwrap()).unwrap();
    assert!(stdin["query"]
      .as_str()
      .unwrap()
      .contains("addPullRequestReviewThreadReply"));
    assert_eq!(stdin["variables"]["threadId"], "PRRT_1");
    assert_eq!(stdin["variables"]["body"], "thanks -- fixed");
  }

  #[test]
  #[cfg(unix)]
  fn test_gh_reply_pr_review_thread_surfaces_graphql_errors() {
    let bin_dir = TempDir::new().unwrap();
    let gh_path = write_stdin_recording_gh(&bin_dir, r#"{"errors":[{"message":"denied"}]}"#);
    let err = gh_reply_pr_review_thread_impl(&gh_path, "T", "b", "/usr/bin:/bin").unwrap_err();
    assert!(err.contains("denied"));
  }

  // ── Reactions ──────────────────────────────────────────────────────────

  #[test]
  fn test_issue_comment_reads_gh_reaction_counts() {
    let comment: GhIssueComment = serde_json::from_value(serde_json::json!({
      "id": "IC_1",
      "body": "hi",
      "author": {"login": "ada"},
      "createdAt": "2026-01-01T00:00:00Z",
      "reactionGroups": [{"content": "HEART", "users": {"totalCount": 2}}]
    }))
    .unwrap();
    assert_eq!(
      comment.reactions,
      vec![GhReactionGroup {
        content: "HEART".into(),
        count: 2,
        viewer_has_reacted: false,
      }]
    );
  }

  #[test]
  #[cfg(unix)]
  fn test_gh_comment_reactions_reads_viewer_flags_and_drops_empty_groups() {
    let bin_dir = TempDir::new().unwrap();
    let gh_path = write_stdin_recording_gh(
      &bin_dir,
      r#"{"data":{"nodes":[{"id":"IC_1","reactionGroups":[{"content":"THUMBS_UP","viewerHasReacted":true,"reactors":{"totalCount":3}},{"content":"EYES","viewerHasReacted":false,"reactors":{"totalCount":0}}]},null]}}"#,
    );

    let by_id = gh_comment_reactions_impl(
      &gh_path,
      &["IC_1".to_string(), "IC_gone".to_string()],
      "/usr/bin:/bin",
    )
    .unwrap();

    assert_eq!(
      by_id["IC_1"],
      vec![GhReactionGroup {
        content: "THUMBS_UP".into(),
        count: 3,
        viewer_has_reacted: true,
      }]
    );
    assert!(!by_id.contains_key("IC_gone"));
    let stdin: serde_json::Value =
      serde_json::from_str(&fs::read_to_string(bin_dir.path().join("stdin")).unwrap()).unwrap();
    assert_eq!(
      stdin["variables"]["ids"],
      serde_json::json!(["IC_1", "IC_gone"])
    );
  }

  #[test]
  #[cfg(unix)]
  fn test_gh_set_reaction_adds_and_removes() {
    let bin_dir = TempDir::new().unwrap();
    let gh_path = write_stdin_recording_gh(&bin_dir, r#"{"data":{}}"#);
    let read_stdin = || -> serde_json::Value {
      serde_json::from_str(&fs::read_to_string(bin_dir.path().join("stdin")).unwrap()).unwrap()
    };

    gh_set_reaction_impl(&gh_path, "PRRC_1", "ROCKET", true, "/usr/bin:/bin").unwrap();
    let stdin = read_stdin();
    assert!(stdin["query"].as_str().unwrap().contains("addReaction"));
    assert_eq!(stdin["variables"]["id"], "PRRC_1");
    assert_eq!(stdin["variables"]["content"], "ROCKET");

    gh_set_reaction_impl(&gh_path, "PRRC_1", "ROCKET", false, "/usr/bin:/bin").unwrap();
    assert!(read_stdin()["query"]
      .as_str()
      .unwrap()
      .contains("removeReaction"));
  }

  #[test]
  #[cfg(unix)]
  fn test_gh_set_reaction_surfaces_graphql_errors() {
    let bin_dir = TempDir::new().unwrap();
    let gh_path = write_stdin_recording_gh(&bin_dir, r#"{"errors":[{"message":"locked"}]}"#);
    let err = gh_set_reaction_impl(&gh_path, "IC_1", "HEART", true, "/usr/bin:/bin").unwrap_err();
    assert!(err.contains("locked"));
  }

  #[test]
  #[cfg(unix)]
  fn test_gh_submit_pr_review_posts_event_body_and_comments() {
    let bin_dir = TempDir::new().unwrap();
    let gh_path = write_stdin_recording_gh(&bin_dir, r#"{"id":1}"#);

    gh_submit_pr_review_impl(
      &gh_path,
      "o/r",
      7,
      "APPROVE",
      "LGTM",
      &[
        GhDraftReviewComment {
          path: "src/a.ts".into(),
          line: 3,
          side: "RIGHT".into(),
          start_line: Some(1),
          body: "nit".into(),
        },
        GhDraftReviewComment {
          path: "src/b.ts".into(),
          line: 9,
          side: "LEFT".into(),
          start_line: None,
          body: "why?".into(),
        },
      ],
      "/usr/bin:/bin",
    )
    .unwrap();

    let args = fs::read_to_string(bin_dir.path().join("args")).unwrap();
    assert_eq!(
      args,
      "api\nrepos/o/r/pulls/7/reviews\n--method\nPOST\n--input\n-\n"
    );
    let stdin: serde_json::Value =
      serde_json::from_str(&fs::read_to_string(bin_dir.path().join("stdin")).unwrap()).unwrap();
    assert_eq!(stdin["event"], "APPROVE");
    assert_eq!(stdin["body"], "LGTM");
    assert_eq!(stdin["comments"][0]["path"], "src/a.ts");
    assert_eq!(stdin["comments"][0]["line"], 3);
    assert_eq!(stdin["comments"][0]["side"], "RIGHT");
    assert_eq!(stdin["comments"][0]["start_line"], 1);
    assert_eq!(stdin["comments"][0]["start_side"], "RIGHT");
    assert!(stdin["comments"][1].get("start_line").is_none());
  }

  #[test]
  #[cfg(unix)]
  fn test_gh_current_user_returns_trimmed_login() {
    let bin_dir = TempDir::new().unwrap();
    let gh_path = write_stdin_recording_gh(&bin_dir, "alice");

    let login = gh_current_user_impl(&gh_path, "/usr/bin:/bin").unwrap();

    assert_eq!(login, "alice");
    let args = fs::read_to_string(bin_dir.path().join("args")).unwrap();
    assert_eq!(args, "api\nuser\n--jq\n.login\n");
  }

  #[test]
  fn test_gh_submit_pr_review_rejects_unknown_event() {
    let err =
      gh_submit_pr_review_impl("gh", "o/r", 7, "MERGE", "", &[], "/usr/bin:/bin").unwrap_err();
    assert!(err.contains("Invalid review event"));
  }

  // ── gh_merge_pr_impl / gh_repo_merge_settings_impl ──────────────────────

  #[test]
  #[cfg(unix)]
  fn merge_pr_passes_method_flag_and_body_on_stdin() {
    let bin_dir = TempDir::new().unwrap();
    let out_dir = TempDir::new().unwrap();
    let stdin_file = out_dir.path().join("stdin");
    let script = format!(
      r#"test "$*" = "pr merge 12 --repo o/r --squash --subject Ship it --body-file -" || exit 9
cat > {}"#,
      stdin_file.display()
    );
    let gh_path = write_fake_gh(&bin_dir, &script);

    gh_merge_pr_impl(
      &gh_path,
      "o/r",
      12,
      GhMergeMethod::Squash,
      Some("Ship it"),
      Some("body text"),
      "/usr/bin:/bin",
    )
    .unwrap();

    assert_eq!(fs::read_to_string(stdin_file).unwrap(), "body text");
  }

  #[test]
  #[cfg(unix)]
  fn merge_pr_rebase_omits_subject_and_body() {
    let bin_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(
      &bin_dir,
      r#"test "$*" = "pr merge 3 --repo o/r --rebase" || exit 9"#,
    );

    gh_merge_pr_impl(
      &gh_path,
      "o/r",
      3,
      GhMergeMethod::Rebase,
      Some("ignored"),
      Some("ignored"),
      "/usr/bin:/bin",
    )
    .unwrap();
  }

  #[test]
  #[cfg(unix)]
  fn merge_pr_reports_failing_checks() {
    let bin_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(
      &bin_dir,
      "echo 'X Pull request o/r#3 is not mergeable: the base branch policy prohibits the merge.' >&2\necho 'Required status check \"ci\" is failing.' >&2\nexit 1",
    );

    let err = gh_merge_pr_impl(
      &gh_path,
      "o/r",
      3,
      GhMergeMethod::Merge,
      None,
      None,
      "/usr/bin:/bin",
    )
    .unwrap_err();

    assert!(err.starts_with("Required checks are failing"), "{err}");
  }

  #[test]
  fn classify_merge_error_detects_conflicts() {
    let err = classify_gh_merge_error(
      "Pull request o/r#3 is not mergeable: the merge commit cannot be cleanly created.",
    );
    assert!(
      err.starts_with("The pull request has merge conflicts"),
      "{err}"
    );
  }

  #[test]
  fn classify_merge_error_detects_not_mergeable() {
    let err = classify_gh_merge_error(
      "Pull request o/r#3 is not mergeable: the base branch policy prohibits the merge.",
    );
    assert!(
      err.starts_with("The pull request is not mergeable"),
      "{err}"
    );
  }

  #[test]
  fn classify_merge_error_detects_disallowed_method() {
    let err = classify_gh_merge_error(
      "GraphQL: Squash merges are not allowed on this repository. (mergePullRequest)",
    );
    assert!(err.starts_with("This merge method is not allowed"), "{err}");
  }

  #[test]
  #[cfg(unix)]
  fn repo_merge_settings_reads_default_and_allowed_methods() {
    let bin_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(
      &bin_dir,
      r#"test "$*" = "repo view o/r --json viewerDefaultMergeMethod,mergeCommitAllowed,squashMergeAllowed,rebaseMergeAllowed" || exit 9
echo '{"viewerDefaultMergeMethod":"SQUASH","mergeCommitAllowed":false,"squashMergeAllowed":true,"rebaseMergeAllowed":true}'"#,
    );

    let settings = gh_repo_merge_settings_impl(&gh_path, "o/r", "/usr/bin:/bin").unwrap();

    assert_eq!(settings.default_method, GhMergeMethod::Squash);
    assert_eq!(
      settings.allowed_methods,
      vec![GhMergeMethod::Squash, GhMergeMethod::Rebase]
    );
  }

  #[test]
  #[cfg(unix)]
  fn repo_merge_settings_falls_back_to_first_allowed_when_default_disallowed() {
    let bin_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(
      &bin_dir,
      r#"echo '{"viewerDefaultMergeMethod":"MERGE","mergeCommitAllowed":false,"squashMergeAllowed":false,"rebaseMergeAllowed":true}'"#,
    );

    let settings = gh_repo_merge_settings_impl(&gh_path, "o/r", "/usr/bin:/bin").unwrap();

    assert_eq!(settings.default_method, GhMergeMethod::Rebase);
  }

  #[test]
  #[cfg(unix)]
  fn set_pr_base_passes_base_flag() {
    let bin_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(
      &bin_dir,
      r#"test "$*" = "pr edit 5 --repo o/r --base main" || exit 9"#,
    );

    gh_set_pr_base_impl(&gh_path, "o/r", 5, "main", "/usr/bin:/bin").unwrap();
  }

  #[test]
  #[cfg(unix)]
  fn retarget_open_prs_moves_only_open_prs_onto_new_base() {
    let bin_dir = TempDir::new().unwrap();
    let repo_dir = TempDir::new().unwrap();
    let log = repo_dir.path().join("edits.log");
    let script = format!(
      r#"case "$*" in
  "pr view --json number,title,state,url,headRefName,baseRefName,mergeStateStatus,isDraft,body -- feat/open")
    echo '{{"number":8,"title":"Open","state":"OPEN","url":"u","headRefName":"feat/open","baseRefName":"feat/parent","mergeStateStatus":null,"isDraft":false}}' ;;
  "pr view --json number,title,state,url,headRefName,baseRefName,mergeStateStatus,isDraft,body -- feat/closed")
    echo '{{"number":9,"title":"Closed","state":"CLOSED","url":"u","headRefName":"feat/closed","baseRefName":"feat/parent","mergeStateStatus":null,"isDraft":false}}' ;;
  "pr view --json number,title,state,url,headRefName,baseRefName,mergeStateStatus,isDraft,body -- feat/none")
    echo 'no pull requests found for branch "feat/none"' >&2; exit 1 ;;
  "pr edit"*) echo "$*" >> {log} ;;
  *) exit 9 ;;
esac"#,
      log = log.display()
    );
    let gh_path = write_fake_gh(&bin_dir, &script);
    let retargets = vec![
      ("feat/open".to_string(), "main".to_string()),
      ("feat/closed".to_string(), "main".to_string()),
      ("feat/none".to_string(), "main".to_string()),
    ];

    let notes = gh_retarget_open_prs_impl(
      &gh_path,
      repo_dir.path().to_str().unwrap(),
      "o/r",
      &retargets,
      "/usr/bin:/bin",
    );

    assert_eq!(notes, vec!["Moved PR #8 (feat/open) onto main".to_string()]);
    assert_eq!(
      fs::read_to_string(log).unwrap().trim(),
      "pr edit 8 --repo o/r --base main"
    );
  }

  #[test]
  #[cfg(unix)]
  fn retarget_open_prs_reports_failed_edit() {
    let bin_dir = TempDir::new().unwrap();
    let repo_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(
      &bin_dir,
      r#"case "$*" in
  "pr view"*) echo '{"number":8,"title":"Open","state":"OPEN","url":"u","headRefName":"feat/open","baseRefName":"feat/parent","mergeStateStatus":null,"isDraft":false}' ;;
  *) echo 'HTTP 422' >&2; exit 1 ;;
esac"#,
    );

    let notes = gh_retarget_open_prs_impl(
      &gh_path,
      repo_dir.path().to_str().unwrap(),
      "o/r",
      &[("feat/open".to_string(), "main".to_string())],
      "/usr/bin:/bin",
    );

    assert_eq!(notes.len(), 1);
    assert!(
      notes[0].starts_with("Could not move PR #8 (feat/open) onto main"),
      "{}",
      notes[0]
    );
  }
}
