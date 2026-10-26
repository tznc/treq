#![expect(
  clippy::print_stdout,
  clippy::print_stderr,
  reason = "the CLI writes to the terminal"
)]

use crate::result_ext::ResultExt;
use args::{Matches, SubcommandMatches};
use serde::Serialize;
use std::path::Path;
use std::time::Duration;

use crate::agent_dispatch;
use crate::core;
use crate::db::Database;
use crate::local_db;

pub(super) fn normalize_repo_path(path: &Path) -> String {
  std::fs::canonicalize(path)
    .ok()
    .and_then(|p| p.to_str().map(|s| s.to_string()))
    .unwrap_or_else(|| path.to_string_lossy().to_string())
}

/// Walk up from CWD to the repository root. See [`find_repo_root`].
pub fn detect_repo_path() -> Result<String, String> {
  let cwd = std::env::current_dir().map_err(|e| format!("Failed to get CWD: {}", e))?;
  find_repo_root(&cwd)
    .map(|root| normalize_repo_path(&root))
    .ok_or_else(|| "Not inside a repository (no .git or .jj found)".to_string())
}

/// The nearest ancestor of `start` (inclusive) that is a repository root: it
/// has `.git` (a directory, or the file a git worktree uses) or `.jj`. Treq's
/// own workspace directories (`.treq/workspaces/<name>`, which hold a `.jj`)
/// are skipped so they resolve to their home repository.
pub(super) fn find_repo_root(start: &Path) -> Option<std::path::PathBuf> {
  start
    .ancestors()
    .find(|dir| {
      !is_treq_workspace_dir(dir) && (dir.join(".git").exists() || dir.join(".jj").is_dir())
    })
    .map(Path::to_path_buf)
}

fn is_treq_workspace_dir(dir: &Path) -> bool {
  let Some(parent) = dir.parent() else {
    return false;
  };
  parent.file_name() == Some(std::ffi::OsStr::new("workspaces"))
    && parent.parent().and_then(Path::file_name) == Some(std::ffi::OsStr::new(".treq"))
}

/// Directory name under `.treq/workspaces/` when `cwd` is inside a workspace.
pub(super) fn workspace_dir_name_from_cwd(cwd: &Path) -> Option<String> {
  let mut current = cwd;
  loop {
    let parent = current.parent()?;
    if parent.file_name() == Some(std::ffi::OsStr::new("workspaces")) {
      let treq = parent.parent()?;
      if treq.file_name() == Some(std::ffi::OsStr::new(".treq")) {
        return current
          .file_name()
          .and_then(|name| name.to_str())
          .map(|name| name.to_string());
      }
    }
    current = parent;
  }
}

/// Repository a workspace command acts on: the repository containing the
/// current directory or, with `-r <repo>`, one of its supporting repositories.
pub(super) fn resolve_command_repo(matches: &Matches) -> Result<String, String> {
  let repo_path = detect_repo_path()?;
  match get_arg_value(matches, "repo") {
    None => Ok(repo_path),
    Some(spec) => {
      let cwd = std::env::current_dir().map_err(|e| format!("Failed to get CWD: {}", e))?;
      core::supporting_repos::resolve(&repo_path, &spec, &cwd)
    }
  }
}

pub(super) fn lookup_workspace_from_cwd(repo_path: &str) -> Option<local_db::Workspace> {
  let cwd = std::env::current_dir().ok()?;
  // With `-r`, the current directory belongs to another repository.
  let cwd_repo = find_repo_root(&cwd).map(|root| normalize_repo_path(&root))?;
  if cwd_repo != normalize_repo_path(Path::new(repo_path)) {
    return None;
  }
  let name = workspace_dir_name_from_cwd(&cwd)?;
  local_db::get_workspace_by_path(repo_path, &name)
    .ok()
    .flatten()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
  Human,
  Json,
}

impl OutputFormat {
  pub(super) fn parse(value: Option<&str>) -> Result<Self, String> {
    match value.unwrap_or("human") {
      "human" => Ok(Self::Human),
      "json" => Ok(Self::Json),
      other => Err(format!("invalid format '{other}'. Expected human or json")),
    }
  }
}

#[derive(Serialize)]
struct CliErrorBody {
  error: CliErrorDetail,
}

#[derive(Serialize)]
struct CliErrorDetail {
  code: String,
  message: String,
}

pub(super) fn print_json<T: Serialize>(value: &T) -> Result<(), String> {
  let json = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
  println!("{json}");
  Ok(())
}

fn write_json_error(out: &mut dyn std::io::Write, code: &str, message: &str) {
  let body = CliErrorBody {
    error: CliErrorDetail {
      code: code.to_string(),
      message: message.to_string(),
    },
  };
  if let Ok(json) = serde_json::to_string(&body) {
    writeln!(out, "{json}").best_effort();
  }
}

/// Runs a command that supports `--format` and reports any error exactly
/// once: a JSON error body on `out` for `--format json`, `Error: ...` on `err`
/// otherwise. Handlers return their errors instead of printing them, so
/// argument errors are never swallowed.
pub(super) fn run_structured_command(
  matches: &Matches,
  out: &mut dyn std::io::Write,
  err: &mut dyn std::io::Write,
  run: impl FnOnce(OutputFormat) -> Result<(), String>,
) -> bool {
  let format = match OutputFormat::parse(get_arg_value(matches, "format").as_deref()) {
    Ok(format) => format,
    Err(error) => {
      writeln!(err, "Error: {error}").best_effort();
      return false;
    }
  };
  match run(format) {
    Ok(()) => true,
    Err(error) => {
      match format {
        OutputFormat::Json => write_json_error(out, classify_cli_error(&error), &error),
        OutputFormat::Human => {
          writeln!(err, "Error: {error}").best_effort();
        }
      }
      false
    }
  }
}

fn run_structured(matches: &Matches, run: impl FnOnce(OutputFormat) -> Result<(), String>) -> bool {
  run_structured_command(matches, &mut std::io::stdout(), &mut std::io::stderr(), run)
}

pub(super) fn get_arg_value(matches: &Matches, name: &str) -> Option<String> {
  matches.args.get(name).and_then(|arg| {
    arg
      .value
      .as_str()
      .filter(|s| !s.is_empty())
      .map(|s| s.to_string())
  })
}

/// Structured error codes are threaded end-to-end: core functions prefix
/// their error strings with one of these codes (`code: message`), and this
/// classifier extracts a matching code even for messages that predate the
/// convention, so a code survives the exec-channel/JSON boundary rather than
/// collapsing to a generic string (PRD: "error codes survive transport
/// mapping").
pub(super) fn classify_cli_error(message: &str) -> &'static str {
  const KNOWN_CODES: &[&str] = &[
    "repository_not_found",
    "invalid_repository",
    "permission_denied",
    "workspace_not_found",
    "agent_already_running",
    "agent_not_running",
    "agent_not_found",
    "dependency_error",
    "filesystem_error",
    "not_implemented",
    "invalid_arguments",
    "ssh_connection_failed",
    "invalid_remote_json",
    "jj_command_failed",
    "git_command_failed",
    "idempotency_conflict",
    "idempotency_ambiguous",
  ];
  for code in KNOWN_CODES {
    if message.starts_with(code) || message.contains(&format!("{code}:")) {
      return code;
    }
  }
  if message.contains("workspace") && message.contains("not found") {
    "workspace_not_found"
  } else if message.contains("jj") {
    "jj_command_failed"
  } else if message.contains("git") {
    "git_command_failed"
  } else {
    "invalid_arguments"
  }
}

fn handle_repo_command(matches: &Matches, format: OutputFormat) -> Result<(), String> {
  let action =
    get_arg_value(matches, "action").ok_or_else(|| "repo action is required".to_string())?;
  let repo_path = get_arg_value(matches, "repo").ok_or_else(|| "--repo is required".to_string())?;

  match action.as_str() {
    "inspect" => {
      let inspection = crate::core::remote::inspect_repository_path(&repo_path)?;
      match format {
        OutputFormat::Json => print_json(&inspection),
        OutputFormat::Human => {
          println!("Repository: {}", inspection.root);
          println!("Type: {}", inspection.repository_type);
          println!(
            "Current branch: {}",
            inspection
              .current_branch
              .unwrap_or_else(|| "detached".to_string())
          );
          println!("Default branch: {}", inspection.default_branch);
          println!("Current commit: {}", inspection.current_commit_id);
          Ok(())
        }
      }
    }
    "status" | "branches" | "probe" | "init" | "clone" | "usage" | "tools" | "dirs"
    | "host-check" | "switch-branch" => handle_remote_review_command("repo", matches, format),
    other => Err(format!("unknown repo action '{other}'")),
  }
}

fn optional_usize(matches: &Matches, name: &str) -> Result<Option<usize>, String> {
  get_arg_value(matches, name)
    .map(|value| {
      value
        .parse()
        .ok()
        .filter(|n| *n > 0)
        .ok_or_else(|| format!("invalid_arguments: --{name} must be a positive integer"))
    })
    .transpose()
}

fn line_range(matches: &Matches) -> Result<(Option<usize>, Option<usize>), String> {
  let start = optional_usize(matches, "start-line")?;
  let end = optional_usize(matches, "end-line")?;
  if let (Some(start), Some(end)) = (start, end) {
    if start > end {
      return Err(format!(
        "invalid_arguments: invalid line range {start}..{end}: expected 1 <= start-line <= end-line"
      ));
    }
  }
  Ok((start, end))
}

fn split_csv(value: Option<String>) -> Vec<String> {
  value
    .map(|value| {
      value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
    })
    .unwrap_or_default()
}

fn require_idempotency_key(key: Option<String>) -> Result<String, String> {
  key
    .filter(|k| !k.trim().is_empty())
    .ok_or_else(|| "invalid_arguments: --idempotency-key is required".to_string())
}

/// `--repo` as an absolute path with `~`, `.`, `..` and trailing slashes
/// resolved lexically. Symlinks are kept, so an already-absolute path, and the
/// DB keys stored under it, stay as they are. Remote commands run without a
/// shell, so nothing else expands a `~` a phone sends.
fn absolute_repo_arg(repo: &str) -> String {
  let repo = crate::core::remote::expand_home(repo, std::env::var("HOME").ok().as_deref());
  let absolute = std::path::absolute(&repo).unwrap_or_else(|_| repo.clone().into());
  let mut normalized = std::path::PathBuf::new();
  for component in absolute.components() {
    match component {
      std::path::Component::CurDir => {}
      std::path::Component::ParentDir => {
        normalized.pop();
      }
      other => normalized.push(other),
    }
  }
  normalized.to_string_lossy().to_string()
}

/// Validates `--metadata` as a JSON object and folds `--title` into it.
fn create_workspace_metadata(
  metadata: Option<String>,
  title: Option<String>,
) -> Result<Option<String>, String> {
  let Some(raw) = metadata.as_deref() else {
    return Ok(title.map(|title| serde_json::json!({ "title": title }).to_string()));
  };
  let Ok(serde_json::Value::Object(mut object)) = serde_json::from_str::<serde_json::Value>(raw)
  else {
    return Err("invalid_arguments: --metadata must be a JSON object".to_string());
  };
  let Some(title) = title else {
    return Ok(metadata);
  };
  object.insert("title".to_string(), serde_json::Value::String(title));
  Ok(Some(serde_json::Value::Object(object).to_string()))
}

pub(crate) fn parse_remote_command_request(
  command: &str,
  matches: &Matches,
) -> Result<crate::core::remote::TreqCommandRequest, String> {
  use crate::core::remote::{FileRevision, TreqCommandRequest};
  let action =
    get_arg_value(matches, "action").ok_or_else(|| format!("{command} action is required"))?;
  let raw_repo = get_arg_value(matches, "repo").ok_or_else(|| "--repo is required".to_string())?;
  let repo = absolute_repo_arg(&raw_repo);
  // A workspace path means its home repo (no stray local.db in the workspace).
  let repo = match (command, action.as_str()) {
    // `~` is the host's home, which only the listing resolves.
    ("repo", "dirs") => raw_repo,
    ("repo", "init" | "clone" | "usage" | "tools") => repo,
    _ => crate::jj::derive_repo_path_from_workspace(&repo).unwrap_or(repo),
  };
  let workspace = get_arg_value(matches, "workspace");
  let path = get_arg_value(matches, "path");
  let target = get_arg_value(matches, "target");
  let value = get_arg_value(matches, "value");
  let idempotency_key = get_arg_value(matches, "idempotency-key");
  let parse_hunks =
    |raw: Option<&String>| -> Result<Vec<crate::core::workspaces::HunkSpec>, String> {
      match raw {
        Some(raw) => raw
          .split(',')
          .map(str::trim)
          .filter(|s| !s.is_empty())
          .map(crate::core::workspaces::parse_hunk_spec)
          .collect(),
        None => Ok(vec![]),
      }
    };
  let require_workspace = || {
    workspace
      .clone()
      .ok_or_else(|| "--workspace is required".to_string())
  };
  let require_value = |what: &str| {
    value
      .clone()
      .ok_or_else(|| format!("--value ({what}) is required"))
  };
  let require_target = |what: &str| {
    target
      .clone()
      .ok_or_else(|| format!("--target ({what}) is required"))
  };
  let (start_line, end_line) = line_range(matches)?;
  match (command, action.as_str()) {
    ("repo", "status") => Ok(TreqCommandRequest::RepositoryStatus { repo }),
    ("repo", "branches") => Ok(TreqCommandRequest::ListBranches { repo }),
    ("repo", "probe") => Ok(TreqCommandRequest::ProbeRepo { repo }),
    ("repo", "usage") => Ok(TreqCommandRequest::MachineUsage { root: repo }),
    ("repo", "tools") => Ok(TreqCommandRequest::MachineTools { root: repo }),
    ("repo", "dirs") => Ok(TreqCommandRequest::ListDirectories { path: repo }),
    ("repo", "host-check") => Ok(TreqCommandRequest::HostCheck { root: repo }),
    ("repo", "switch-branch") => Ok(TreqCommandRequest::SwitchRepoBranch {
      repo,
      bookmark: require_value("branch name")?,
    }),
    ("repo", "init") => Ok(TreqCommandRequest::InitRepo {
      repo,
      idempotency_key: require_idempotency_key(idempotency_key)?,
    }),
    ("repo", "clone") => Ok(TreqCommandRequest::CloneRepo {
      repo_url: require_value("repository URL")?,
      destination: repo,
      idempotency_key: require_idempotency_key(idempotency_key)?,
    }),
    ("workspace", "list") => Ok(TreqCommandRequest::ListWorkspaces { repo }),
    ("workspace", "summaries") => Ok(TreqCommandRequest::WorkspaceSummaries { repo }),
    ("workspace", "plan-new") => Ok(TreqCommandRequest::PlanNewWorkspaces {
      repo,
      text: require_value("lines")?,
      stack_on: target,
    }),
    ("workspace", "new") => Ok(TreqCommandRequest::NewWorkspaces {
      repo,
      text: require_value("lines")?,
      stack_on: target,
      idempotency_key: require_idempotency_key(idempotency_key)?,
    }),
    ("workspace", "inspect") => Ok(TreqCommandRequest::InspectWorkspace {
      repo,
      workspace: require_workspace()?,
    }),
    ("workspace", "create") => Ok(TreqCommandRequest::CreateWorkspace {
      repo,
      branch_name: require_value("branch name")?,
      source_branch: target,
      metadata: create_workspace_metadata(
        get_arg_value(matches, "metadata"),
        get_arg_value(matches, "title"),
      )?,
      idempotency_key: require_idempotency_key(idempotency_key)?,
    }),
    ("workspace", "rename") => Ok(TreqCommandRequest::RenameWorkspace {
      repo,
      workspace: require_workspace()?,
      new_name: require_value("new branch name")?,
      idempotency_key: require_idempotency_key(idempotency_key)?,
    }),
    ("workspace", "update") => Ok(TreqCommandRequest::UpdateWorkspace {
      repo,
      workspace: require_workspace()?,
      target_branch: target,
      title: get_arg_value(matches, "title"),
      description: value,
    }),
    ("workspace", "delete") => Ok(TreqCommandRequest::DeleteWorkspace {
      repo,
      workspace: require_workspace()?,
    }),
    ("workspace", "archive") => Ok(TreqCommandRequest::ArchiveWorkspace {
      repo,
      workspace: require_workspace()?,
    }),
    ("workspace", "move") => Ok(TreqCommandRequest::MoveWorkspaceChanges {
      repo,
      workspace: require_workspace()?,
      destination: require_target("destination workspace/branch")?,
      files: split_csv(get_arg_value(matches, "files")),
      hunks: parse_hunks(path.as_ref())?,
      commits: split_csv(value),
      idempotency_key: require_idempotency_key(idempotency_key)?,
    }),
    ("workspace", "rebase") => Ok(TreqCommandRequest::RebaseWorkspace {
      repo,
      workspace: require_workspace()?,
      target_branch: require_target("target branch")?,
      idempotency_key: require_idempotency_key(idempotency_key)?,
    }),
    ("workspace", "snapshot") => Ok(TreqCommandRequest::SnapshotWorkingCopy { repo, workspace }),
    ("workspace", "restore-all") => Ok(TreqCommandRequest::RestoreAll { repo, workspace }),
    ("workspace", "restore-snapshot") => Ok(TreqCommandRequest::RestoreSnapshot {
      repo,
      workspace,
      snapshot_id: require_target("snapshot id")?,
    }),
    ("changes", "list") => Ok(TreqCommandRequest::ListChanges { repo, workspace }),
    ("changes", "review") => Ok(TreqCommandRequest::ReviewLoad {
      repo,
      workspace: require_workspace()?,
      scope: core::remote::ReviewScope::parse(&require_target("review scope")?)?,
    }),
    ("changes", "review-save") => Ok(TreqCommandRequest::ReviewSave {
      repo,
      workspace: require_workspace()?,
      scope: core::remote::ReviewScope::parse(&require_target("review scope")?)?,
      review: serde_json::from_str(&require_value("review JSON")?)
        .map_err(|e| format!("invalid_arguments: --value must be a review: {e}"))?,
    }),
    ("changes", "review-clear") => Ok(TreqCommandRequest::ReviewClear {
      repo,
      workspace: require_workspace()?,
      scope: core::remote::ReviewScope::parse(&require_target("review scope")?)?,
    }),
    ("changes", "workspace-diff") => Ok(TreqCommandRequest::WorkspaceDiff {
      repo,
      workspace: require_workspace()?,
    }),
    ("commits", "diff") => Ok(TreqCommandRequest::CommitDiff {
      repo,
      workspace,
      revision: require_target("revision")?,
    }),
    ("commits", "file-diff") => Ok(TreqCommandRequest::CommitFileDiff {
      repo,
      workspace,
      revision: require_target("revision")?,
      path: path.ok_or_else(|| "--path is required".to_string())?,
    }),
    ("file", "search") => Ok(TreqCommandRequest::SearchFiles {
      repo,
      workspace,
      query: value.unwrap_or_default(),
      limit: optional_usize(matches, "limit")?,
    }),
    ("file", "list") => Ok(TreqCommandRequest::ListFiles {
      repo,
      workspace,
      path: path.unwrap_or_default(),
    }),
    ("changes", "diff") => Ok(TreqCommandRequest::DiffFile {
      repo,
      workspace,
      path: path.ok_or_else(|| "--path is required".to_string())?,
    }),
    ("changes", "diff-files") => Ok(TreqCommandRequest::DiffFiles {
      repo,
      workspace,
      paths: serde_json::from_str(
        &get_arg_value(matches, "paths").ok_or_else(|| "--paths is required".to_string())?,
      )
      .map_err(|e| format!("invalid_arguments: --paths must be a JSON list of paths: {e}"))?,
      token: get_arg_value(matches, "token"),
    }),
    ("file", "read") => Ok(TreqCommandRequest::ReadFile {
      repo,
      workspace,
      path: path.ok_or_else(|| "--path is required".to_string())?,
      revision: match get_arg_value(matches, "revision")
        .as_deref()
        .unwrap_or("working-copy")
      {
        "working-copy" => FileRevision::WorkingCopy,
        "parent" => FileRevision::Parent,
        _ => return Err("--revision must be working-copy or parent".to_string()),
      },
      start_line,
      end_line,
    }),
    ("file", "restore") => Ok(TreqCommandRequest::RestoreFile {
      repo,
      workspace,
      path: path.ok_or_else(|| "--path is required".to_string())?,
    }),
    ("file", "patch") => Ok(TreqCommandRequest::PatchFile {
      repo,
      workspace,
      path: path.ok_or_else(|| "--path is required".to_string())?,
      patch_base64: require_value("base64 patch content")?,
      idempotency_key: require_idempotency_key(idempotency_key)?,
    }),
    ("commits", "list") => Ok(TreqCommandRequest::ListCommits { repo, workspace }),
    ("workspace", "stash") => Ok(TreqCommandRequest::StashWorkspaceChanges {
      repo,
      workspace,
      idempotency_key: require_idempotency_key(idempotency_key)?,
    }),
    ("commits", "create") => Ok(TreqCommandRequest::CreateCommit {
      repo,
      workspace,
      message: require_value("commit message")?,
      base_change_id: target,
      idempotency_key: require_idempotency_key(idempotency_key)?,
    }),
    ("commits", "describe") => Ok(TreqCommandRequest::DescribeCommit {
      repo,
      workspace: require_workspace()?,
      commit: require_target("commit change id")?,
      message: require_value("description")?,
    }),
    ("commits", "split") => Ok(TreqCommandRequest::SplitCommit {
      repo,
      workspace: require_workspace()?,
      commit: require_target("commit change id")?,
      files: split_csv(value),
      hunks: parse_hunks(path.as_ref())?,
      message: get_arg_value(matches, "message").unwrap_or_default(),
      idempotency_key: require_idempotency_key(idempotency_key)?,
    }),
    ("commits", "move") => Ok(TreqCommandRequest::MoveCommit {
      repo,
      workspace: require_workspace()?,
      commit: require_target("commit change id")?,
      target_workspace: require_value("target workspace id")?,
      idempotency_key: require_idempotency_key(idempotency_key)?,
    }),
    ("commits", "undo") => Ok(TreqCommandRequest::UndoCommit {
      repo,
      workspace: require_workspace()?,
      commit: require_target("commit change id")?,
    }),
    ("commits", "abandon") => Ok(TreqCommandRequest::AbandonCommit {
      repo,
      workspace: require_workspace()?,
      commit: require_target("commit change id")?,
      idempotency_key: require_idempotency_key(idempotency_key)?,
    }),
    ("commits", "undo-operation") => Ok(TreqCommandRequest::UndoOperation {
      repo,
      workspace,
      operation_id: require_target("operation id")?,
    }),
    ("commits", "revert") => Ok(TreqCommandRequest::RevertCommit {
      repo,
      workspace: require_workspace()?,
      commit: require_target("commit change id")?,
      idempotency_key: require_idempotency_key(idempotency_key)?,
    }),
    ("conflicts", "list") => Ok(TreqCommandRequest::ListConflicts { repo, workspace }),
    ("workspace", "marker") => Ok(TreqCommandRequest::WorkspaceChangeMarker { repo, workspace }),
    ("conflicts", "resolve") => Ok(TreqCommandRequest::ResolveConflict {
      repo,
      revision: require_target("revision")?,
      sides: split_csv(value),
      idempotency_key: require_idempotency_key(idempotency_key)?,
    }),
    ("git", "fetch") => Ok(TreqCommandRequest::GitFetch { repo }),
    ("git", "pull") => Ok(TreqCommandRequest::PullWorkspace { repo, workspace }),
    ("git", "remote-info") => Ok(TreqCommandRequest::GitRemoteInfo { repo }),
    ("git", "bookmark-track") => Ok(TreqCommandRequest::GitBookmarkTrack {
      repo,
      bookmark: require_value("bookmark name")?,
      remote_name: require_target("remote name")?,
    }),
    ("git", "push") => Ok(TreqCommandRequest::GitPush {
      repo,
      workspace,
      idempotency_key: require_idempotency_key(idempotency_key)?,
    }),
    ("git", "pr") => Ok(TreqCommandRequest::WorkspacePullRequest {
      repo,
      workspace: require_workspace()?,
    }),
    ("git", "pr-create") => Ok(TreqCommandRequest::CreatePullRequest {
      repo,
      workspace: require_workspace()?,
      title: get_arg_value(matches, "title").ok_or("--title is required")?,
      body: value.unwrap_or_default(),
      draft: get_arg_value(matches, "draft").as_deref() == Some("true"),
      idempotency_key: require_idempotency_key(idempotency_key)?,
    }),
    ("git", "github-read") => Ok(TreqCommandRequest::GithubRead {
      repo,
      query: serde_json::from_str(&require_value("GitHub query JSON")?)
        .map_err(|e| format!("invalid GitHub query: {e}"))?,
    }),
    ("git", "github-write") => Ok(TreqCommandRequest::GithubWrite {
      repo,
      action: serde_json::from_str(&require_value("GitHub action JSON")?)
        .map_err(|e| format!("invalid GitHub action: {e}"))?,
      idempotency_key: require_idempotency_key(idempotency_key)?,
    }),
    ("agent-remote", "start") => Ok(TreqCommandRequest::AgentStart {
      repo,
      workspace: require_workspace()?,
      agent: require_target("agent name")?,
      prompt: require_value("prompt")?,
      idempotency_key: require_idempotency_key(idempotency_key)?,
    }),
    ("agent-remote", "input") => Ok(TreqCommandRequest::AgentInput {
      repo,
      workspace: require_workspace()?,
      input: require_value("input")?,
      idempotency_key: require_idempotency_key(idempotency_key)?,
    }),
    ("agent-remote", "status") => Ok(TreqCommandRequest::AgentStatus {
      repo,
      workspace: require_workspace()?,
    }),
    ("agent-remote", "stop") => Ok(TreqCommandRequest::AgentStop {
      repo,
      workspace: require_workspace()?,
    }),
    ("agent-remote", "logs") => Ok(TreqCommandRequest::AgentLogs {
      repo,
      workspace: require_workspace()?,
    }),
    ("pty-remote", "start") => {
      let backend = pty_backend(matches)?;
      let payload = core::remote::parse_pty_launch_payload(&require_value("pty launch payload")?)?;
      Ok(TreqCommandRequest::PtyStart {
        backend,
        repo,
        workspace: require_workspace()?,
        label: require_target("session label")?,
        remote_dir: payload.remote_dir,
        launch: payload.launch,
        cols: payload.cols,
        rows: payload.rows,
        idempotency_key: require_idempotency_key(idempotency_key)?,
      })
    }
    ("pty-remote", "list") => Ok(TreqCommandRequest::PtyList {
      backend: pty_backend(matches)?,
      repo,
      workspace,
    }),
    ("pty-remote", "status") => Ok(TreqCommandRequest::PtyStatus),
    ("pty-remote", "reset") => Ok(TreqCommandRequest::PtyReset {
      backend: pty_backend(matches)?,
    }),
    ("pty-remote", "paste") => Ok(TreqCommandRequest::PtyPaste {
      backend: pty_backend(matches)?,
      repo,
      workspace: require_workspace()?,
      label: require_target("session label")?,
      text: require_value("text to paste")?,
      submit: get_arg_value(matches, "submit").as_deref() == Some("true"),
      idempotency_key: require_idempotency_key(idempotency_key)?,
    }),
    ("pty-remote", "stop") => Ok(TreqCommandRequest::PtyStop {
      backend: pty_backend(matches)?,
      repo,
      workspace: require_workspace()?,
      label: require_target("session label")?,
    }),
    ("pty-remote", "rename") => Ok(TreqCommandRequest::PtyRename {
      backend: pty_backend(matches)?,
      repo,
      workspace: require_workspace()?,
      label: require_target("session label")?,
      // An empty or missing value clears the title.
      title: value.clone().unwrap_or_default(),
    }),
    ("pty-remote", "attach-command") => {
      let backend = pty_backend(matches)?;
      let payload = core::remote::parse_pty_launch_payload(&require_value("pty launch payload")?)?;
      Ok(TreqCommandRequest::PtyAttachCommand {
        backend,
        repo,
        workspace: require_workspace()?,
        label: require_target("session label")?,
        remote_dir: payload.remote_dir,
        launch: payload.launch,
        cols: payload.cols,
        rows: payload.rows,
      })
    }
    _ => Err(format!("unknown {command} action '{action}'")),
  }
}

fn pty_backend(matches: &Matches) -> Result<core::pty_remote_supervisor::PtyBackend, String> {
  core::pty_remote_supervisor::PtyBackend::parse(get_arg_value(matches, "backend").as_deref())
}

/// `pty-remote` actions that hold the terminal instead of printing a
/// result: `daemon` (serves until reset or idle) and `attach` (what an SSH
/// PTY channel runs, from `attach-command`).
fn interactive_pty_action(matches: &Matches) -> Option<String> {
  get_arg_value(matches, "action").filter(|action| action == "daemon" || action == "attach")
}

fn handle_interactive_pty(action: &str, matches: &Matches) -> bool {
  let result = match action {
    "daemon" => core::pty_remote_supervisor::run_daemon(),
    _ => run_pty_attach(matches),
  };
  match result {
    Ok(()) => true,
    Err(error) => {
      eprintln!("treq pty-remote {action}: {error}");
      false
    }
  }
}

fn run_pty_attach(matches: &Matches) -> Result<(), String> {
  let required = |name: &str, what: &str| {
    get_arg_value(matches, name).ok_or_else(|| format!("invalid_arguments: {what} is required"))
  };
  let repo = required("repo", "repo")?;
  let workspace = required("workspace", "workspace")?;
  let label = required("target", "session label")?;
  let payload = core::remote::parse_pty_launch_payload(&required("value", "pty launch payload")?)?;
  core::pty_remote_supervisor::run_daemon_attach(
    &repo,
    &payload.remote_dir,
    &workspace,
    &label,
    &payload.launch,
    payload.cols,
    payload.rows,
  )
}

fn handle_remote_review_command(
  command: &str,
  matches: &Matches,
  format: OutputFormat,
) -> Result<(), String> {
  let request = parse_remote_command_request(command, matches)?;
  let status = matches!(request, core::remote::TreqCommandRequest::PtyStatus);
  let value = crate::core::remote::execute_local_request(request)?;
  match format {
    OutputFormat::Json => print_json(&value),
    OutputFormat::Human if status => {
      let status: core::pty_remote_supervisor::TerminalStatus =
        serde_json::from_value(value).map_err(|e| e.to_string())?;
      println!("{}", core::pty_remote_supervisor::describe_status(&status));
      Ok(())
    }
    OutputFormat::Human => {
      println!("{value:#}");
      Ok(())
    }
  }
}

/// Top-level CLI dispatch. Returns the process exit code if the subcommand was
/// recognized, or `None` if it was not (caller should print usage and exit).
pub fn handle_cli_command(subcommand: &SubcommandMatches) -> Option<i32> {
  let success = match subcommand.name.as_str() {
    "add" => workspace_handlers::handle_workspace_add(&subcommand.matches),
    "set" => workspace_handlers::handle_workspace_set(&subcommand.matches),
    "st" => workspace_handlers::handle_workspace_status(&subcommand.matches),
    "diff" => workspace_handlers::handle_workspace_diff(&subcommand.matches),
    "mv" => workspace_handlers::handle_workspace_move(&subcommand.matches),
    "agent" => workspace_handlers::handle_workspace_agent(&subcommand.matches),
    "commit" => workspace_handlers::handle_workspace_commit(&subcommand.matches),
    "resolve" => workspace_handlers::handle_resolve(&subcommand.matches),
    "send" => workspace_handlers::handle_send(&subcommand.matches),
    "notify" => workspace_handlers::handle_notify(&subcommand.matches),
    "repo" => run_structured(&subcommand.matches, |format| {
      handle_repo_command(&subcommand.matches, format)
    }),
    "agent-review" => run_structured(&subcommand.matches, |format| {
      agent_review_handlers::handle_agent_review_command(&subcommand.matches, format)
    }),
    "pty-remote" if interactive_pty_action(&subcommand.matches).is_some() => {
      let action = interactive_pty_action(&subcommand.matches).unwrap_or_default();
      handle_interactive_pty(&action, &subcommand.matches)
    }
    "workspace" | "changes" | "file" | "commits" | "conflicts" | "git" | "agent-remote"
    | "pty-remote" => run_structured(&subcommand.matches, |format| {
      handle_remote_review_command(&subcommand.name, &subcommand.matches, format)
    }),
    _ => return None,
  };
  Some(if success { 0 } else { 1 })
}

/// How long a command waits for piped stdin it did not explicitly ask for.
const IMPLICIT_STDIN_WAIT: Duration = Duration::from_secs(1);

/// `true` when `fd` has data or EOF within `timeout`. Agent harnesses often
/// run commands with stdin as a pipe nobody writes to or closes; reading it to
/// EOF would hang forever.
#[cfg(unix)]
pub(super) fn fd_has_input(fd: std::os::unix::io::RawFd, timeout: Duration) -> bool {
  let mut poll_fd = libc::pollfd {
    fd,
    events: libc::POLLIN,
    revents: 0,
  };
  let timeout_ms = timeout.as_millis().min(i32::MAX as u128) as i32;
  // SAFETY: `poll_fd` is a live, initialised `pollfd` and the count is 1, so
  // `poll` reads and writes only that one struct.
  let ready = unsafe { libc::poll(&mut poll_fd, 1, timeout_ms) };
  ready > 0 && poll_fd.revents & (libc::POLLIN | libc::POLLHUP) != 0
}

/// Whether to read stdin a command did not explicitly ask for (no `-`
/// argument): it must be redirected and deliver data or EOF promptly.
pub(super) fn implicit_stdin_available() -> bool {
  use std::io::IsTerminal;
  if std::io::stdin().is_terminal() {
    return false;
  }
  #[cfg(unix)]
  {
    use std::os::unix::io::AsRawFd;
    fd_has_input(std::io::stdin().as_raw_fd(), IMPLICIT_STDIN_WAIT)
  }
  #[cfg(not(unix))]
  {
    true
  }
}

/// Prints an error to stderr and records it in the app's log file.
pub(super) fn log_cli_error(msg: &str) {
  eprintln!("{}", msg);
  tracing::error!("{}", msg);
}

#[cfg(test)]
pub(super) fn is_supported_cli_command(name: &str) -> bool {
  matches!(
    name,
    "add"
      | "set"
      | "st"
      | "diff"
      | "mv"
      | "agent"
      | "commit"
      | "resolve"
      | "send"
      | "notify"
      | "repo"
      | "workspace"
      | "changes"
      | "file"
      | "commits"
      | "conflicts"
      | "git"
      | "agent-remote"
      | "agent-review"
      | "pty-remote"
  )
}

pub(super) fn parse_agent_mode(mode: &str) -> Result<&'static str, String> {
  match mode.trim() {
    "edit" => Ok("acceptEdits"),
    "plan" => Ok("plan"),
    other => Err(format!(
      "invalid mode '{}'. Expected one of: edit, plan",
      other
    )),
  }
}

pub(super) fn parse_agent_mode_or_default(mode: Option<&str>) -> Result<&'static str, String> {
  match mode {
    Some(value) => parse_agent_mode(value),
    None => Ok("acceptEdits"),
  }
}

fn get_db_setting(db: &Database, key: &str) -> Option<String> {
  db.get_setting(key)
    .ok()
    .flatten()
    .map(|v| v.trim().to_string())
    .filter(|v| !v.is_empty())
}

pub(super) fn resolve_default_agent(repo_path: &str) -> String {
  let db_path = core::resolve_app_db_path(repo_path);
  let Ok(db) = Database::new(db_path) else {
    return "claude".to_string();
  };

  db.get_repo_setting(repo_path, "default_agent")
    .ok()
    .flatten()
    .map(|v| v.trim().to_string())
    .filter(|v| !v.is_empty())
    .or_else(|| get_db_setting(&db, "default_agent"))
    .unwrap_or_else(|| "claude".to_string())
}

pub(super) fn dispatch_agent_request(
  repo_path: &str,
  branch: &str,
  prompt: &str,
  mode: &str,
  agent: &str,
  request_id: &str,
) -> Result<(), String> {
  let now = agent_dispatch::now_millis();
  local_db::prune_stale_instance_registry(repo_path, now, agent_dispatch::HEARTBEAT_TIMEOUT_MS)?;
  let instances = local_db::list_instance_registry(repo_path)?;

  let instance =
    agent_dispatch::resolve_target_instance(&instances, repo_path).ok_or_else(|| {
      format!(
        "No running Treq instance has repo '{}'. Open this repo in Treq first.",
        repo_path
      )
    })?;

  let request = agent_dispatch::AgentDispatchRequest {
    request_id: request_id.to_string(),
    repo: repo_path.to_string(),
    branch: branch.to_string(),
    prompt: prompt.to_string(),
    mode: mode.to_string(),
    agent: agent.to_string(),
  };
  let response = agent_dispatch::send_dispatch_request(
    &instance.endpoint,
    &request,
    Duration::from_millis(250),
    Duration::from_millis(600),
  )?;
  if response.status == "handled" {
    return Ok(());
  }
  Err(format!(
    "Agent request not handled by instance '{}': {}",
    instance.instance_id,
    response
      .reason
      .unwrap_or_else(|| "unknown dispatch failure".to_string())
  ))
}

/// Sends a JSON IPC request to the Treq instance that has `repo` open.
fn dispatch_to_repo_instance<T: serde::Serialize>(
  repo: &str,
  request: &T,
) -> Result<(String, agent_dispatch::AgentDispatchResponse), String> {
  let now = agent_dispatch::now_millis();
  local_db::prune_stale_instance_registry(repo, now, agent_dispatch::HEARTBEAT_TIMEOUT_MS)?;
  let instances = local_db::list_instance_registry(repo)?;

  let instance = agent_dispatch::resolve_target_instance(&instances, repo).ok_or_else(|| {
    format!(
      "No running Treq instance has repo '{}'. Open this repo in Treq first.",
      repo
    )
  })?;

  let response = send_json_dispatch_request(
    &instance.endpoint,
    request,
    Duration::from_millis(250),
    Duration::from_millis(600),
  )?;
  Ok((instance.instance_id.clone(), response))
}

pub(super) fn dispatch_send_request(
  request: &crate::send_dispatch::SendDispatchRequest,
) -> Result<(), String> {
  let (instance_id, response) = dispatch_to_repo_instance(&request.repo, request)?;
  if response.status == "handled" {
    return Ok(());
  }
  Err(format!(
    "Send request not handled by instance '{}': {}",
    instance_id,
    response
      .reason
      .unwrap_or_else(|| "unknown dispatch failure".to_string())
  ))
}

/// `Ok(None)` when the app showed the notification, `Ok(Some(reason))` when
/// it chose not to (for example because a Treq window is focused).
pub(super) fn dispatch_notify_request(
  request: &crate::notify_dispatch::NotifyDispatchRequest,
) -> Result<Option<String>, String> {
  let (instance_id, response) = dispatch_to_repo_instance(&request.repo, request)?;
  match response.status.as_str() {
    "handled" => Ok(None),
    "skipped" => Ok(Some(response.reason.unwrap_or_default())),
    _ => Err(format!(
      "Notify request not handled by instance '{}': {}",
      instance_id,
      response
        .reason
        .unwrap_or_else(|| "unknown dispatch failure".to_string())
    )),
  }
}

fn send_json_dispatch_request<T: serde::Serialize>(
  endpoint: &str,
  request: &T,
  connect_timeout: Duration,
  io_timeout: Duration,
) -> Result<agent_dispatch::AgentDispatchResponse, String> {
  use std::io::{Read, Write};
  use std::net::{Shutdown, TcpStream};

  let addr = endpoint
    .parse()
    .map_err(|e| format!("invalid endpoint '{}': {}", endpoint, e))?;
  let mut stream = TcpStream::connect_timeout(&addr, connect_timeout)
    .map_err(|e| format!("failed to connect to endpoint {}: {}", endpoint, e))?;

  stream
    .set_read_timeout(Some(io_timeout))
    .map_err(|e| format!("failed to set read timeout: {}", e))?;
  stream
    .set_write_timeout(Some(io_timeout))
    .map_err(|e| format!("failed to set write timeout: {}", e))?;

  let payload = serde_json::to_vec(request)
    .map_err(|e| format!("failed to serialize dispatch request: {}", e))?;
  stream
    .write_all(&payload)
    .map_err(|e| format!("failed to send dispatch request: {}", e))?;
  stream
    .shutdown(Shutdown::Write)
    .map_err(|e| format!("failed to finish dispatch write: {}", e))?;

  let mut response = String::new();
  stream
    .read_to_string(&mut response)
    .map_err(|e| format!("failed reading dispatch response: {}", e))?;

  serde_json::from_str::<agent_dispatch::AgentDispatchResponse>(response.trim())
    .map_err(|e| format!("invalid dispatch response payload: {}", e))
}

pub mod args;
mod workspace_handlers;

mod agent_review_handlers;
mod status_output;

#[cfg(test)]
mod tests;
