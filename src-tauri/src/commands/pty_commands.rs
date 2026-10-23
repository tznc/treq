use crate::AppState;
use std::path::{Path, PathBuf};

fn resolve_local_working_dir(
  repo_path: Option<&str>,
  workspace_id: Option<i64>,
  working_dir_override: Option<String>,
) -> Result<Option<String>, String> {
  if let Some(working_dir) = working_dir_override {
    return Ok(Some(working_dir));
  }
  let Some(repo_path) = repo_path else {
    return Ok(None);
  };
  let Some(workspace_id) = workspace_id else {
    return Ok(Some(repo_path.to_string()));
  };
  let workspace = crate::local_db::get_workspace_by_id(repo_path, workspace_id)?
    .ok_or_else(|| format!("Workspace {workspace_id} not found"))?;
  let stored_path = Path::new(&workspace.workspace_path);
  let absolute_path = if stored_path.is_absolute() {
    stored_path.to_path_buf()
  } else if stored_path.starts_with(".treq/workspaces") {
    Path::new(repo_path).join(stored_path)
  } else {
    PathBuf::from(repo_path)
      .join(".treq/workspaces")
      .join(stored_path)
  };
  Ok(Some(absolute_path.to_string_lossy().into_owned()))
}

/// The repository and working directory for a PTY that should run in the
/// repository's devcontainer, or `None` to run it on the host. The repo's
/// `environment` setting decides, and only while the preview is on.
fn devcontainer_repo(
  state: &AppState,
  repo_path: Option<&str>,
  working_dir: Option<&str>,
) -> Option<(String, String)> {
  use crate::core::devcontainer::{repo_root_for, ENVIRONMENT_DEVCONTAINER, ENVIRONMENT_SETTING};
  use crate::core::feature_preview::{self, PreviewFeature};
  use crate::lock_ext::LockExt;

  let dir = working_dir.or(repo_path)?.to_string();
  let repo = match repo_path {
    Some(repo) => repo.to_string(),
    None => repo_root_for(Path::new(&dir))
      .to_string_lossy()
      .into_owned(),
  };
  let db = state.db.lock_or_recover();
  if !feature_preview::is_enabled(&db, PreviewFeature::Devcontainers) {
    return None;
  }
  let environment = db.get_repo_setting(&repo, ENVIRONMENT_SETTING).ok()??;
  drop(db);
  (environment == ENVIRONMENT_DEVCONTAINER).then_some((repo, dir))
}

/// What a PTY session runs and where.
pub(crate) struct Launch {
  pub shell: Option<String>,
  pub shell_args: Vec<String>,
  pub working_dir: Option<String>,
  pub initial_command: Option<String>,
}

/// Resolves a session's shell and directory: over ssh for a remote host, in
/// the repo's devcontainer when it uses one, else on the host in the
/// workspace's directory.
#[allow(
  clippy::too_many_arguments,
  reason = "flat argument list kept to match callers"
)]
pub(crate) fn launch(
  state: &AppState,
  session_id: &str,
  remote_host: Option<&str>,
  repo_path: Option<&str>,
  workspace_id: Option<i64>,
  working_dir: Option<String>,
  shell: Option<String>,
  initial_command: Option<String>,
) -> Result<Launch, String> {
  if let Some(host) = remote_host {
    let (program, args) = crate::core::remote::build_ssh_shell_command(
      host,
      working_dir.as_deref(),
      initial_command.as_deref(),
    )?;
    return Ok(Launch {
      shell: Some(program),
      shell_args: args,
      working_dir: None,
      initial_command: None,
    });
  }
  let working_dir = resolve_local_working_dir(repo_path, workspace_id, working_dir)?;
  Ok(
    match devcontainer_repo(state, repo_path, working_dir.as_deref()) {
      Some((repo, dir)) => {
        let (program, args) =
          crate::core::devcontainer::exec_shell_command(&repo, &dir, session_id);
        Launch {
          shell: Some(program),
          shell_args: args,
          working_dir: None,
          initial_command,
        }
      }
      None => Launch {
        shell,
        shell_args: Vec::new(),
        working_dir,
        initial_command,
      },
    },
  )
}

#[cfg(test)]
mod tests {
  use super::*;
  use tempfile::TempDir;

  #[test]
  fn resolves_workspace_id_to_absolute_working_directory() {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = temp.path().to_str().expect("utf8 path");
    crate::local_db::init_local_db(repo_path).expect("initialize database");
    let workspace_id = crate::local_db::add_workspace(
      repo_path,
      "feature-one".to_string(),
      "feature-one".to_string(),
      "feature/one".to_string(),
      None,
      None,
      None,
    )
    .expect("register workspace");

    assert_eq!(
      resolve_local_working_dir(Some(repo_path), Some(workspace_id), None).unwrap(),
      Some(
        temp
          .path()
          .join(".treq/workspaces/feature-one")
          .to_string_lossy()
          .into_owned()
      )
    );
  }

  #[test]
  fn resolves_missing_workspace_id_to_home_repository() {
    assert_eq!(
      resolve_local_working_dir(Some("/repo"), None, None).unwrap(),
      Some("/repo".to_string())
    );
  }

  #[test]
  fn preserves_explicit_working_directory_override() {
    assert_eq!(
      resolve_local_working_dir(
        Some("/repo"),
        Some(7),
        Some("/repo/.treq/resolve/change".to_string())
      )
      .unwrap(),
      Some("/repo/.treq/resolve/change".to_string())
    );
  }
}
