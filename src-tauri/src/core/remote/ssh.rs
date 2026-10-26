use super::{
  fs, jj, BTreeSet, Path, PathBuf, RepositoryDescriptor, RepositoryInspection, RepositoryLocation,
  SshHost,
};

pub fn inspect_repository_path(repo_path: &str) -> Result<RepositoryInspection, String> {
  let root = validate_repository_path(repo_path)?;
  let repository_type = detect_repository_type(&root)?;
  let branch = crate::core::repo::get_repo_current_branch(&root).ok();
  let default_branch =
    crate::core::repo::get_repo_default_branch(&root).unwrap_or_else(|_| "main".to_string());
  let current_commit_id = jj::jj_get_commit_id(&root, "@").unwrap_or_default();
  let current_change_id = String::new();
  let display_name = repository_display_name(&root);

  Ok(RepositoryInspection {
    root: root.clone(),
    repository_type,
    current_branch: branch.and_then(|branch| branch.current_branch),
    default_branch,
    current_change_id,
    current_commit_id,
    descriptor: RepositoryDescriptor {
      id: format!("local:{root}"),
      location: RepositoryLocation::Local { path: root },
      display_name,
    },
  })
}

pub fn parse_ssh_config_hosts(contents: &str) -> Vec<SshHost> {
  let mut aliases = BTreeSet::new();
  for line in contents.lines() {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
      continue;
    }
    let mut parts = trimmed.split_whitespace();
    let Some(keyword) = parts.next() else {
      continue;
    };
    if !keyword.eq_ignore_ascii_case("host") {
      continue;
    }
    for alias in parts {
      if alias == "*" || alias.contains('*') || alias.contains('?') || alias.starts_with('!') {
        continue;
      }
      aliases.insert(alias.to_string());
    }
  }
  aliases.into_iter().map(|alias| SshHost { alias }).collect()
}

pub fn list_configured_hosts() -> Result<Vec<SshHost>, String> {
  list_configured_hosts_from_paths(ssh_config_paths())
}

pub fn list_configured_hosts_from_paths(paths: Vec<PathBuf>) -> Result<Vec<SshHost>, String> {
  let mut all = BTreeSet::new();
  for path in paths {
    if let Ok(contents) = fs::read_to_string(path) {
      for host in parse_ssh_config_hosts(&contents) {
        all.insert(host.alias);
      }
    }
  }
  Ok(all.into_iter().map(|alias| SshHost { alias }).collect())
}

pub fn build_ssh_shell_command(
  host: &str,
  working_dir: Option<&str>,
  initial_command: Option<&str>,
) -> Result<(String, Vec<String>), String> {
  validate_host_alias(host)?;
  let mut remote_command = String::new();
  if let Some(dir) = working_dir {
    validate_remote_path(dir)?;
    remote_command.push_str("cd ");
    remote_command.push_str(&shell_quote(dir));
    remote_command.push_str(" && ");
  }
  remote_command.push_str("${SHELL:-/bin/sh} -l");
  if let Some(command) = initial_command {
    if !command.trim().is_empty() {
      remote_command.push_str(" -c ");
      remote_command.push_str(&shell_quote(command));
    }
  }
  Ok((
    "ssh".to_string(),
    // `--` stops ssh reading the host as an option.
    vec![
      "-t".to_string(),
      "--".to_string(),
      host.to_string(),
      remote_command,
    ],
  ))
}

pub(super) fn validate_repository_path(repo_path: &str) -> Result<String, String> {
  let trimmed = repo_path.trim();
  if trimmed.is_empty() {
    return Err("Repository path is required".to_string());
  }
  let path = Path::new(trimmed);
  if !path.exists() {
    return Err("repository_not_found: Repository path does not exist".to_string());
  }
  if !path.is_dir() {
    return Err("invalid_repository: Repository path is not a directory".to_string());
  }
  std::fs::canonicalize(path)
    .map_err(|e| format!("permission_denied: Failed to read repository path: {e}"))?
    .to_str()
    .map(|path| path.to_string())
    .ok_or_else(|| "invalid_repository: Repository path is not valid UTF-8".to_string())
}

pub(super) fn detect_repository_type(root: &str) -> Result<String, String> {
  let path = Path::new(root);
  let has_jj = path.join(".jj").is_dir();
  let has_git = path.join(".git").exists();
  match (has_jj, has_git) {
    (true, true) => Ok("jj_colocated".to_string()),
    (true, false) => Ok("jj".to_string()),
    (false, true) => Ok("git".to_string()),
    (false, false) => Err("invalid_repository: No .jj or .git directory found".to_string()),
  }
}

pub(super) fn repository_display_name(path: &str) -> String {
  Path::new(path)
    .file_name()
    .and_then(|name| name.to_str())
    .filter(|name| !name.is_empty())
    .unwrap_or(path)
    .to_string()
}

pub(super) fn ssh_config_paths() -> Vec<PathBuf> {
  let mut paths = Vec::new();
  if let Ok(home) = std::env::var("HOME") {
    paths.push(PathBuf::from(home).join(".ssh").join("config"));
  }
  paths.push(PathBuf::from("/etc/ssh/ssh_config"));
  paths
}

pub fn shell_quote(value: &str) -> String {
  format!("'{}'", value.replace('\'', "'\\''"))
}

pub(super) fn validate_host_alias(host: &str) -> Result<(), String> {
  if host.trim().is_empty() {
    return Err("SSH host is required".to_string());
  }
  if host.starts_with('-')
    || !host
      .chars()
      .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '@' | ':' | '-'))
  {
    return Err("SSH host must be a host alias from ssh config".to_string());
  }
  Ok(())
}

pub(super) fn validate_remote_path(path: &str) -> Result<(), String> {
  if path.trim().is_empty() {
    return Err("Remote path is required".to_string());
  }
  Ok(())
}
