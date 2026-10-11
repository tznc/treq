use crate::AppState;
use std::path::{Path, PathBuf};
use tauri::{Emitter, State};

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
  state: &State<'_, AppState>,
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
  (environment == ENVIRONMENT_DEVCONTAINER).then_some((repo, dir))
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn pty_create_session(
  state: State<'_, AppState>,
  session_id: String,
  window_label: Option<String>,
  working_dir: Option<String>,
  repo_path: Option<String>,
  workspace_id: Option<i64>,
  shell: Option<String>,
  initial_command: Option<String>,
  suppress_echo_for: Option<String>,
  remote_host: Option<String>,
) -> Result<(), String> {
  log::debug!(
        "pty_create_session: session_id={}, working_dir={:?}, shell={:?}, initial_command_present={}, suppress_echo_for_present={}",
        session_id,
        working_dir,
        shell,
        initial_command.is_some(),
        suppress_echo_for.is_some()
    );
  let pty_manager = state.pty_manager.clone();
  let sid = session_id.clone();
  let event_name = format!("pty-data-{}", sid);
  let emit_output: Box<dyn Fn(String) + Send + 'static> =
    match state.watcher_manager.cloned_app_handle() {
      Some(app) => Box::new(move |data| {
        if let Err(error) = app.emit(&event_name, data) {
          log::warn!(
            "pty emit failed: session_id={}, event={}, error={}",
            sid,
            event_name,
            error
          );
        }
      }),
      // Integration tests have no Tauri AppHandle. They skip the PTY spawn
      // unless a test opts in, and then read output from the test sink.
      #[cfg(feature = "tauri-test")]
      None if crate::test_pty_events::pty_enabled() => {
        Box::new(move |data| crate::test_pty_events::push(&event_name, data))
      }
      None => return Ok(()),
    };

  let (shell, shell_args, working_dir, initial_command) = if let Some(host) = remote_host {
    let (program, args) = crate::core::remote::build_ssh_shell_command(
      &host,
      working_dir.as_deref(),
      initial_command.as_deref(),
    )?;
    (Some(program), args, None, None)
  } else {
    let working_dir = resolve_local_working_dir(repo_path.as_deref(), workspace_id, working_dir)?;
    match devcontainer_repo(&state, repo_path.as_deref(), working_dir.as_deref()) {
      Some((repo, dir)) => {
        let (program, args) =
          crate::core::devcontainer::exec_shell_command(&repo, &dir, &session_id);
        (Some(program), args, None, initial_command)
      }
      None => (shell, Vec::new(), working_dir, initial_command),
    }
  };

  tauri::async_runtime::spawn_blocking(move || {
    pty_manager.create_session(
      session_id,
      window_label,
      working_dir,
      shell,
      shell_args,
      initial_command,
      suppress_echo_for,
      emit_output,
    )
  })
  .await
  .map_err(|error| error.to_string())?
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

#[tauri::command]
pub fn pty_session_exists(state: State<AppState>, session_id: String) -> Result<bool, String> {
  Ok(state.pty_manager.session_exists(&session_id))
}

#[tauri::command]
pub fn pty_write(state: State<AppState>, session_id: String, data: String) -> Result<(), String> {
  log::debug!(
    "pty_write: session_id={}, data_len={}",
    session_id,
    data.len()
  );
  state.pty_manager.write_to_session(&session_id, &data)
}

#[tauri::command]
pub fn pty_resize(
  state: State<AppState>,
  session_id: String,
  rows: u16,
  cols: u16,
) -> Result<(), String> {
  log::debug!(
    "pty_resize: session_id={}, rows={}, cols={}",
    session_id,
    rows,
    cols
  );
  state.pty_manager.resize_session(&session_id, rows, cols)
}

#[tauri::command]
pub fn pty_write_suppress_echo(
  state: State<AppState>,
  session_id: String,
  data: String,
) -> Result<(), String> {
  log::debug!(
    "pty_write_suppress_echo: session_id={}, data_len={}",
    session_id,
    data.len()
  );
  state.pty_manager.set_auto_command(&session_id, &data)?;
  state.pty_manager.write_to_session(&session_id, &data)
}

#[tauri::command]
pub async fn pty_close(state: State<'_, AppState>, session_id: String) -> Result<(), String> {
  log::debug!("pty_close: session_id={}", session_id);
  let pty_manager = state.pty_manager.clone();
  tauri::async_runtime::spawn_blocking(move || pty_manager.close_session(&session_id))
    .await
    .map_err(|error| error.to_string())?
}
