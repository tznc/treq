use std::path::Path;

use tauri_plugin_cli::Matches;

use crate::core;
use crate::db::Database;
use crate::local_db;

use super::status_output::{
  filter_statuses_for_workspace, format_focused_workspace_status, format_workspace_diff_lines,
  format_workspace_stack_lines, print_workspace_partial_status, WorkspacePrStatus,
};
use super::{
  detect_repo_path, dispatch_agent_request, dispatch_notify_request, dispatch_send_request,
  parse_agent_mode_or_default, resolve_default_agent,
};
use crate::notify_dispatch::{normalize_message, NotifyDispatchRequest, NotifyEvent, NOTIFY_KIND};

fn get_arg_value(matches: &Matches, name: &str) -> Option<String> {
  matches.args.get(name).and_then(|arg| {
    arg
      .value
      .as_str()
      .filter(|s| !s.is_empty())
      .map(|s| s.to_string())
  })
}

fn get_arg_flag(matches: &Matches, name: &str) -> bool {
  matches
    .args
    .get(name)
    .and_then(|arg| arg.value.as_bool())
    .unwrap_or(false)
}

fn github_pr_status(repo_path: &str, branch_name: &str) -> Option<WorkspacePrStatus> {
  let remote = crate::github::get_git_remote_url_impl(repo_path).ok()??;
  let gh = crate::binary_paths::get_binary_path("gh")
    .or_else(|| crate::binary_paths::detect_binary("gh"))?;
  let path = crate::binary_paths::get_extended_path();
  let pr = crate::github::get_pr_info_via_gh_impl(&gh, repo_path, branch_name, &path).ok()??;
  let checks = crate::github::get_pr_checks_via_gh_impl(&gh, repo_path, branch_name, &path)
    .ok()
    .flatten();
  Some(WorkspacePrStatus {
    github_id: format!("{}#{}", remote.full_name, pr.number),
    checks,
  })
}

fn get_arg_values(matches: &Matches, name: &str) -> Vec<String> {
  let Some(arg) = matches.args.get(name) else {
    return Vec::new();
  };
  match &arg.value {
    serde_json::Value::Array(values) => values
      .iter()
      .filter_map(|value| value.as_str())
      .map(|value| value.to_string())
      .filter(|value| !value.is_empty())
      .collect(),
    serde_json::Value::String(value) if !value.is_empty() => vec![value.to_string()],
    _ => Vec::new(),
  }
}

pub(super) fn handle_workspace_add(matches: &Matches) -> bool {
  let branch_name = match get_arg_value(matches, "branch_name") {
    Some(name) => name,
    None => {
      super::log_cli_error("Error: branch name is required");
      eprintln!(
                "Usage: treq add <branch_name> [-d description] [-l title] [-s source_branch] [-p sparse_path]... [-k symlink_path]..."
            );
      return false;
    }
  };

  let description = get_arg_value(matches, "description");
  let title = get_arg_value(matches, "title");
  let source_branch = get_arg_value(matches, "source-branch");
  let sparse_patterns = get_arg_values(matches, "sparse");
  let sparse_patterns = (!sparse_patterns.is_empty()).then_some(sparse_patterns);
  let symlinked_dirs = get_arg_values(matches, "symlink");
  let symlinked_dirs = (!symlinked_dirs.is_empty()).then_some(symlinked_dirs);

  let repo_path = match super::resolve_command_repo(matches) {
    Ok(p) => p,
    Err(e) => {
      super::log_cli_error(&format!("Error: {}", e));
      return false;
    }
  };

  // Ensure the repo is initialized
  if let Err(e) = core::init(&repo_path) {
    super::log_cli_error(&format!("Error initializing repo: {}", e));
    return false;
  }

  // Same settings the app applies when it creates a workspace.
  let included_copy_files = Database::new(core::resolve_app_db_path(&repo_path))
    .ok()
    .and_then(|db| {
      db.get_repo_setting(&repo_path, "included_copy_files")
        .ok()
        .flatten()
    });
  let metadata = core::WorkspaceMetadata {
    title,
    description,
    sparse_patterns,
    symlinked_dirs: symlinked_dirs.clone(),
    ..Default::default()
  };

  match core::create_new_workspace(
    &repo_path,
    &branch_name,
    source_branch.as_deref(),
    metadata,
    core::parse_included_copy_files(included_copy_files.as_deref()),
  ) {
    Ok(workspace) => {
      println!("Created workspace: {}", workspace.branch_name);
      if workspace.title != workspace.branch_name {
        println!("  Title: {}", workspace.title);
      }
      if let Some(ref description) = workspace.description {
        println!("  Description: {}", description);
      }
      let full_path = Path::new(&repo_path)
        .join(".treq")
        .join("workspaces")
        .join(&workspace.workspace_path);
      if let Some(ref dirs) = symlinked_dirs {
        let (linked, missing) = split_symlinks(&full_path, dirs);
        if !linked.is_empty() {
          println!("  Symlinked: {}", linked.join(", "));
        }
        for dir in missing {
          eprintln!("Warning: '{dir}' was not symlinked; it does not exist in the home repo");
        }
      }
      println!("  Path: {}", full_path.display());
      run_setup_script_for_cli(&repo_path, workspace.id, &full_path.to_string_lossy())
    }
    Err(e) => {
      super::log_cli_error(&format!("Error creating workspace: {}", e));
      false
    }
  }
}

/// Splits the requested `-k` paths into those now symlinked in the workspace
/// and those that were not (their source does not exist in the home repo).
pub(super) fn split_symlinks(
  workspace_path: &Path,
  requested: &[String],
) -> (Vec<String>, Vec<String>) {
  requested.iter().cloned().partition(|dir| {
    workspace_path
      .join(dir)
      .symlink_metadata()
      .is_ok_and(|meta| meta.file_type().is_symlink())
  })
}

/// Runs the repo's setup script in the foreground. The app runs it in the
/// background, but the CLI process exits right after `add`, and checks stay
/// blocked for a workspace whose setup script never ran.
fn run_setup_script_for_cli(repo_path: &str, workspace_id: i64, workspace_path: &str) -> bool {
  let script = match core::configured_setup_script(repo_path) {
    Ok(Some(script)) => script,
    Ok(None) => return true,
    Err(e) => {
      super::log_cli_error(&format!("Error reading setup_script: {}", e));
      return false;
    }
  };
  println!("Running setup script...");
  match core::run_setup_script_sync(repo_path, workspace_id, workspace_path, &script) {
    Ok(result) if result.success => {
      println!("  Setup script finished");
      true
    }
    Ok(_) => {
      super::log_cli_error("Setup script failed; see the workspace's Checks logs");
      false
    }
    Err(e) => {
      super::log_cli_error(&format!("Error running setup script: {}", e));
      false
    }
  }
}

pub(super) fn handle_workspace_set(matches: &Matches) -> bool {
  let workspace_name = match get_arg_value(matches, "workspace_name") {
    Some(name) => name,
    None => {
      super::log_cli_error("Error: workspace name is required");
      eprintln!("Usage: treq set <workspace_name> [-d description] [-l title] [-t target_branch]");
      return false;
    }
  };

  let description = get_arg_value(matches, "description");
  let title = get_arg_value(matches, "title");
  let target_branch = get_arg_value(matches, "target-branch");

  if description.is_none() && target_branch.is_none() && title.is_none() {
    super::log_cli_error(
      "Error: specify at least one of -d (description), -l (title), or -t (target branch)",
    );
    return false;
  }

  let repo_path = match super::resolve_command_repo(matches) {
    Ok(p) => p,
    Err(e) => {
      super::log_cli_error(&format!("Error: {}", e));
      return false;
    }
  };

  // Look up workspace by branch name
  let workspace = match local_db::get_workspace_by_branch(&repo_path, &workspace_name) {
    Ok(Some(ws)) => ws,
    Ok(None) => {
      super::log_cli_error(&format!("Error: workspace '{}' not found", workspace_name));
      return false;
    }
    Err(e) => {
      super::log_cli_error(&format!("Error looking up workspace: {}", e));
      return false;
    }
  };

  let description_param = match description {
    Some(i) => core::MaybeEmptyParam::Some(i),
    None => core::MaybeEmptyParam::Omitted,
  };
  let title_param = match title {
    Some(t) => core::MaybeEmptyParam::Some(t),
    None => core::MaybeEmptyParam::Omitted,
  };

  let target_param = match target_branch {
    Some(t) => core::MaybeEmptyParam::Some(t),
    None => core::MaybeEmptyParam::Omitted,
  };

  match core::update_workspace_with_title(
    &repo_path,
    workspace.id,
    target_param,
    title_param,
    description_param,
  ) {
    Ok(updated) => {
      println!("Updated workspace: {}", updated.branch_name);
      println!("  Title: {}", updated.title);
      if let Some(ref description) = updated.description {
        println!("  Description: {}", description);
      }
      if let Some(ref target) = updated.target_branch {
        println!("  Target: {}", target);
      }
      true
    }
    Err(e) => {
      super::log_cli_error(&format!("Error updating workspace: {}", e));
      false
    }
  }
}

fn resolve_named_or_cwd_workspace(
  repo_path: &str,
  workspace_name: Option<String>,
) -> Result<local_db::Workspace, String> {
  if let Some(name) = workspace_name {
    return local_db::get_workspace_by_branch(repo_path, &name)?
      .ok_or_else(|| format!("workspace '{}' not found", name));
  }
  super::lookup_workspace_from_cwd(repo_path).ok_or_else(|| {
    "run this command from a workspace directory, or pass a workspace name".to_string()
  })
}

fn default_branch_or_empty(repo_path: &str) -> String {
  core::get_repo_default_branch(repo_path).unwrap_or_default()
}

fn print_focused_workspace(repo_path: &str, workspace: &local_db::Workspace) -> bool {
  let default_branch = default_branch_or_empty(repo_path);
  let status = match core::workspace_status(repo_path, Some(workspace.id)) {
    Ok(status) => status,
    Err(e) => {
      super::log_cli_error(&format!("Error getting workspace status: {}", e));
      return false;
    }
  };
  let related = match core::list_workspace_statuses(repo_path) {
    Ok(statuses) => {
      filter_statuses_for_workspace(&statuses, &workspace.branch_name, &default_branch)
    }
    Err(e) => {
      super::log_cli_error(&format!("Error listing workspace statuses: {}", e));
      return false;
    }
  };
  let uncommitted_changes = core::list_changed_files(repo_path, Some(workspace.id))
    .map(|files| files.len())
    .unwrap_or(0);
  let pr = github_pr_status(repo_path, &status.partial.current.branch_name);
  for line in format_focused_workspace_status(
    &status,
    &related,
    uncommitted_changes,
    &default_branch,
    pr.as_ref(),
  ) {
    println!("{line}");
  }
  true
}

pub(super) fn handle_workspace_status(matches: &Matches) -> bool {
  let workspace_name = get_arg_value(matches, "workspace_name");

  let repo_path = match super::resolve_command_repo(matches) {
    Ok(p) => p,
    Err(e) => {
      super::log_cli_error(&format!("Error: {}", e));
      return false;
    }
  };

  if workspace_name.is_some() || super::lookup_workspace_from_cwd(&repo_path).is_some() {
    match resolve_named_or_cwd_workspace(&repo_path, workspace_name) {
      Ok(workspace) => print_focused_workspace(&repo_path, &workspace),
      Err(e) => {
        super::log_cli_error(&format!("Error: {}", e));
        false
      }
    }
  } else {
    let default_branch = default_branch_or_empty(&repo_path);
    match core::list_workspace_statuses(&repo_path) {
      Ok(statuses) => {
        if statuses.is_empty() {
          println!("No workspaces found.");
          return true;
        }
        for line in format_workspace_stack_lines(&statuses) {
          println!("{line}");
        }
        println!("Details:");
        for status in &statuses {
          let pr = github_pr_status(&repo_path, &status.current.branch_name);
          print_workspace_partial_status(status, pr.as_ref(), &default_branch);
        }
        true
      }
      Err(e) => {
        super::log_cli_error(&format!("Error listing workspace statuses: {}", e));
        false
      }
    }
  }
}

pub(super) fn handle_workspace_diff(matches: &Matches) -> bool {
  let workspace_name = get_arg_value(matches, "workspace_name");
  let repo_path = match super::resolve_command_repo(matches) {
    Ok(p) => p,
    Err(e) => {
      super::log_cli_error(&format!("Error: {}", e));
      return false;
    }
  };

  let workspace = match resolve_named_or_cwd_workspace(&repo_path, workspace_name) {
    Ok(workspace) => workspace,
    Err(e) => {
      super::log_cli_error(&format!("Error: {}", e));
      return false;
    }
  };

  let diff = match core::workspace_cli_diff(&repo_path, workspace.id) {
    Ok(diff) => diff,
    Err(e) => {
      super::log_cli_error(&format!("Error getting workspace diff: {}", e));
      return false;
    }
  };

  let conflicted_commits =
    match core::list_commits(&repo_path, Some(workspace.id), false, None, Some(50)) {
      Ok(log) => log
        .commits
        .into_iter()
        .filter(|commit| commit.has_conflicts && !commit.is_working_copy)
        .map(|commit| (commit.change_id, commit.description))
        .collect::<Vec<_>>(),
      Err(_) => Vec::new(),
    };

  for line in format_workspace_diff_lines(&diff, &conflicted_commits) {
    println!("{line}");
  }
  true
}

pub(super) fn handle_workspace_move(matches: &Matches) -> bool {
  let source = match get_arg_value(matches, "source") {
    Some(value) => value,
    None => {
      super::log_cli_error("Error: source workspace is required");
      eprintln!(
                "Usage: treq mv <source> <destination> -f [FILES...] -r [RANGES...] -c [COMMITS...]  (use '.' for the home repo)"
            );
      return false;
    }
  };
  let destination = match get_arg_value(matches, "destination") {
    Some(value) => value,
    None => {
      super::log_cli_error("Error: destination workspace is required");
      eprintln!(
                "Usage: treq mv <source> <destination> -f [FILES...] -r [RANGES...] -c [COMMITS...]  (use '.' for the home repo)"
            );
      return false;
    }
  };

  let files = get_arg_values(matches, "files");
  let commits = get_arg_values(matches, "commits");
  let raw_hunks = get_arg_values(matches, "ranges");
  let mut hunks = Vec::new();
  for raw_hunk in raw_hunks {
    match core::parse_hunk_spec(&raw_hunk) {
      Ok(spec) => hunks.push(spec),
      Err(error) => {
        super::log_cli_error(&format!("Error: {}", error));
        return false;
      }
    }
  }

  let request = core::WorkspaceMoveRequest {
    files,
    hunks,
    commits,
  };
  if !request.has_selectors() {
    super::log_cli_error("Error: specify at least one of -f, -r, or -c");
    return false;
  }

  let repo_path = match super::resolve_command_repo(matches) {
    Ok(path) => path,
    Err(error) => {
      super::log_cli_error(&format!("Error: {}", error));
      return false;
    }
  };

  match core::move_workspace_changes(&repo_path, &source, &destination, request) {
    Ok(result) => {
      println!(
        "Moved changes from '{}' to '{}': commits={}, files={}, hunks_applied={}, hunks_skipped={}",
        source,
        destination,
        result.commits_moved,
        result.files_moved,
        result.hunks_applied,
        result.hunks_skipped
      );
      for warning in result.warnings {
        eprintln!("Warning: {}", warning);
        tracing::warn!("{}", warning);
      }
      true
    }
    Err(error) => {
      super::log_cli_error(&format!("Error moving workspace changes: {}", error));
      false
    }
  }
}

pub(super) fn handle_workspace_agent(matches: &Matches) -> bool {
  let branch = match get_arg_value(matches, "branch") {
    Some(value) => value,
    None => {
      super::log_cli_error("Error: branch is required");
      eprintln!("Usage: treq agent <branch> <prompt> [-m <edit|plan>]");
      return false;
    }
  };

  let prompt = match get_arg_value(matches, "prompt") {
    Some(value) => value,
    None => {
      super::log_cli_error("Error: prompt is required");
      eprintln!("Usage: treq agent <branch> <prompt> [-m <edit|plan>]");
      return false;
    }
  };

  if branch == "." {
    super::log_cli_error(
      "Error: agents cannot be started in a home repository. Create a workspace with `treq add` first.",
    );
    return false;
  }

  let mode = match parse_agent_mode_or_default(get_arg_value(matches, "mode").as_deref()) {
    Ok(mode) => mode.to_string(),
    Err(error) => {
      super::log_cli_error(&format!("Error: {}", error));
      return false;
    }
  };

  let repo_path = match super::resolve_command_repo(matches) {
    Ok(path) => path,
    Err(error) => {
      super::log_cli_error(&format!("Error: {}", error));
      return false;
    }
  };

  if let Err(error) = core::init(&repo_path) {
    super::log_cli_error(&format!("Error initializing repo: {}", error));
    return false;
  }

  let workspace = match local_db::get_workspace_by_branch(&repo_path, &branch) {
    Ok(Some(workspace)) => workspace,
    Ok(None) => {
      super::log_cli_error(&format!(
        "Error: workspace branch '{}' not found. Create it first with `treq add {}`.",
        branch, branch
      ));
      return false;
    }
    Err(error) => {
      super::log_cli_error(&format!("Error looking up workspace: {}", error));
      return false;
    }
  };

  let request_id = format!(
    "cli-{}-{}",
    workspace.id,
    chrono::Utc::now().timestamp_millis()
  );
  let agent = resolve_default_agent(&repo_path);
  if let Err(error) = dispatch_agent_request(
    &repo_path,
    &workspace.branch_name,
    &prompt,
    &mode,
    &agent,
    &request_id,
  ) {
    super::log_cli_error(&format!("Error dispatching agent request: {}", error));
    return false;
  }
  true
}

pub(super) fn handle_workspace_commit(matches: &Matches) -> bool {
  let workspace_name = match get_arg_value(matches, "workspace_name") {
    Some(value) => value,
    None => {
      super::log_cli_error("Error: workspace name is required");
      eprintln!("Usage: treq commit <workspace_name> -m <message> [--push]");
      return false;
    }
  };

  let message = match get_arg_value(matches, "message") {
    Some(value) => value,
    None => {
      super::log_cli_error("Error: commit message is required (-m)");
      eprintln!("Usage: treq commit <workspace_name> -m <message> [--push]");
      return false;
    }
  };

  let push = get_arg_flag(matches, "push");

  let repo_path = match super::resolve_command_repo(matches) {
    Ok(path) => path,
    Err(error) => {
      super::log_cli_error(&format!("Error: {}", error));
      return false;
    }
  };

  let workspace = match local_db::get_workspace_by_branch(&repo_path, &workspace_name) {
    Ok(Some(ws)) => ws,
    Ok(None) => {
      super::log_cli_error(&format!("Error: workspace '{}' not found", workspace_name));
      return false;
    }
    Err(error) => {
      super::log_cli_error(&format!("Error looking up workspace: {}", error));
      return false;
    }
  };

  match commit_workspace_for_cli(&repo_path, &workspace, &message, push) {
    Ok(lines) => {
      for line in lines {
        println!("{line}");
      }
      true
    }
    Err(error) => {
      super::log_cli_error(&format!("Error: {}", error));
      false
    }
  }
}

/// `treq commit` with its outcome spelled out: no empty commits, and a failed
/// `--push` says the commit itself landed.
pub(super) fn commit_workspace_for_cli(
  repo_path: &str,
  workspace: &local_db::Workspace,
  message: &str,
  push: bool,
) -> Result<Vec<String>, String> {
  if message.trim().is_empty() {
    return Err("commit message must not be blank".to_string());
  }
  let mut lines = vec![
    core::commit_workspace_changes(repo_path, Some(workspace.id), message).map_err(|e| match e
      .as_str()
    {
      core::NOTHING_TO_COMMIT => {
        format!("nothing to commit in workspace '{}'", workspace.branch_name)
      }
      _ => format!("failed to create commit: {e}"),
    })?,
  ];
  if push {
    let pushed = core::push_workspace_to_remote(repo_path, Some(workspace.id)).map_err(|e| {
      format!(
        "committed to '{}', but push failed: {e}",
        workspace.branch_name
      )
    })?;
    lines.push(pushed);
  }
  Ok(lines)
}

pub(super) fn handle_resolve(matches: &Matches) -> bool {
  use std::collections::HashMap;
  use std::io::Read;

  let commit_id = match get_arg_value(matches, "commit_id") {
    Some(value) => value,
    None => {
      super::log_cli_error("Error: commit id is required");
      eprintln!("Usage: treq resolve <commit_id> [sides...]");
      return false;
    }
  };

  let side_tokens = get_arg_values(matches, "sides");
  let sides = match core::parse_resolve_sides(&side_tokens) {
    Ok(parsed) => parsed,
    Err(error) => {
      super::log_cli_error(&format!("Error: {}", error));
      return false;
    }
  };

  let repo_path = match detect_repo_path() {
    Ok(path) => path,
    Err(error) => {
      super::log_cli_error(&format!("Error: {}", error));
      return false;
    }
  };

  if let Err(error) = core::init(&repo_path) {
    super::log_cli_error(&format!("Error initializing repo: {}", error));
    return false;
  }

  let mut replacements: Option<HashMap<String, String>> = None;
  if super::implicit_stdin_available() {
    let mut stdin_body = String::new();
    if std::io::stdin().read_to_string(&mut stdin_body).is_ok() {
      let trimmed = stdin_body.trim();
      if !trimmed.is_empty() {
        match serde_json::from_str::<HashMap<String, String>>(trimmed) {
          Ok(map) => replacements = Some(map),
          Err(error) => {
            super::log_cli_error(&format!("Error: invalid stdin JSON: {}", error));
            return false;
          }
        }
      }
    }
  }

  match core::resolve_commit(&repo_path, &commit_id, &sides, replacements) {
    Ok(result) => {
      if result.success {
        println!("{}", result.message);
        true
      } else {
        super::log_cli_error(&result.message);
        false
      }
    }
    Err(error) => {
      super::log_cli_error(&format!("Error: {}", error));
      false
    }
  }
}

/// Validates `treq notify` arguments: a message, or `--agent-exited` alone.
pub(super) fn parse_notify_args(
  matches: &Matches,
) -> Result<(NotifyEvent, Option<String>), String> {
  let message = get_arg_value(matches, "message");
  if get_arg_flag(matches, "agent-exited") {
    return match message {
      Some(_) => Err("pass a message or --agent-exited, not both".to_string()),
      None => Ok((NotifyEvent::AgentExited, None)),
    };
  }
  let message = message
    .as_deref()
    .and_then(normalize_message)
    .ok_or_else(|| "a message is required".to_string())?;
  Ok((NotifyEvent::Message, Some(message)))
}

pub(super) fn handle_notify(matches: &Matches) -> bool {
  let (event, message) = match parse_notify_args(matches) {
    Ok(parsed) => parsed,
    Err(error) => {
      super::log_cli_error(&format!("Error: {}", error));
      eprintln!("Usage: treq notify <message>");
      return false;
    }
  };
  let repo_path = match detect_repo_path() {
    Ok(path) => path,
    Err(error) => {
      super::log_cli_error(&format!("Error: {}", error));
      return false;
    }
  };
  // Resolve like `treq st`, else the directory name of an unregistered workspace.
  let workspace = super::lookup_workspace_from_cwd(&repo_path)
    .map(|workspace| workspace.branch_name)
    .or_else(|| {
      let cwd = std::env::current_dir().ok()?;
      super::workspace_dir_name_from_cwd(&cwd)
    });
  let request = NotifyDispatchRequest {
    kind: NOTIFY_KIND.to_string(),
    request_id: format!("notify-{}", chrono::Utc::now().timestamp_millis()),
    repo: repo_path,
    workspace,
    event,
    message,
  };
  match dispatch_notify_request(&request) {
    Ok(None) => println!("Notification shown"),
    Ok(Some(reason)) => println!("Notification skipped: {reason}"),
    Err(error) => {
      super::log_cli_error(&format!("Error dispatching notification: {}", error));
      return false;
    }
  }
  true
}

pub(super) fn handle_send(matches: &Matches) -> bool {
  let path_arg = get_arg_value(matches, "path");
  let browser_mode = get_arg_flag(matches, "browser");

  let repo_path = match detect_repo_path() {
    Ok(path) => path,
    Err(error) => {
      super::log_cli_error(&format!("Error: {}", error));
      return false;
    }
  };

  if let Err(error) = core::init(&repo_path) {
    super::log_cli_error(&format!("Error initializing repo: {}", error));
    return false;
  }

  let (path, media_type, title) = if browser_mode {
    let Some(target) = path_arg.as_deref() else {
      super::log_cli_error("Error: --browser requires a path or URL argument");
      eprintln!("Usage: treq send --browser <path-or-url>");
      return false;
    };
    match crate::send_dispatch::resolve_browser_send_target(target) {
      Ok(url) => (
        url.clone(),
        crate::send_dispatch::MEDIA_BROWSER.to_string(),
        url,
      ),
      Err(error) => {
        super::log_cli_error(&format!("Error: {}", error));
        eprintln!("Usage: treq send --browser <path-or-url>");
        return false;
      }
    }
  } else {
    // With no path, read stdin only if it has input; `-` always reads it.
    let is_stdin_tty = path_arg.is_none() && !super::implicit_stdin_available();
    let mut stdin = std::io::stdin();
    match crate::send_dispatch::resolve_send_path(
      &repo_path,
      path_arg.as_deref(),
      &mut stdin,
      is_stdin_tty,
    ) {
      Ok((path, media_type, title)) => (
        path.to_string_lossy().to_string(),
        media_type.to_string(),
        title,
      ),
      Err(error) => {
        super::log_cli_error(&format!("Error: {}", error));
        eprintln!("Usage: treq send [path|-]");
        return false;
      }
    }
  };

  let request_id = format!("send-{}", chrono::Utc::now().timestamp_millis());
  let mut request =
    crate::send_dispatch::SendDispatchRequest::new(request_id, &repo_path, media_type, path);
  request.title = Some(title);
  request.pty_session_id = crate::send_dispatch::pty_session_id_from_env();

  if let Err(error) = crate::send_dispatch::record_send_artifact(&request) {
    super::log_cli_error(&format!("Error recording send artifact: {}", error));
    return false;
  }

  if let Err(error) = dispatch_send_request(&request) {
    super::log_cli_error(&format!("Error dispatching send request: {}", error));
    return false;
  }

  println!("Sent {} ({})", request.path, request.media_type);
  true
}
