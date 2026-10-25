//! What the window does for the remote actions its screens and dialogs
//! send, as Dashboard's remote handlers did (`useGpuiRemoteDialogs`,
//! `handleGpuiRemotePress`, `gpuiRemoteRepo`, `resolveRemoteShell`,
//! `startRemoteAgent`), and what a remote repository opening does to the
//! window.

use serde_json::{json, Value};

use super::{
  data::Data,
  model::{full_path, Effect, Model, Page, WorkspaceView},
  remote::{self, Opened, RemoteEffect, UserManagedValues},
};
use crate::{
  core::remote_pty::RemoteAgentId,
  gpui_shell::{native::ToastTone, screen::account_data::CloudAction},
  local_db::Workspace,
};

fn remote(effects: Vec<RemoteEffect>) -> Vec<Effect> {
  effects.into_iter().map(Effect::from).collect()
}

fn toast(tone: ToastTone, title: &str, description: &str) -> Effect {
  Effect::Toast {
    tone,
    title: title.into(),
    description: description.into(),
  }
}

impl Model {
  /// The window shows a remote repository.
  pub fn is_remote(&self) -> bool {
    self.remote.repository.is_some()
  }

  /// `handleOpenRemoteSetup`: the dialog rereads the cloud workspace as it
  /// opens.
  pub fn open_remote_setup(&mut self) {
    self.remote.setup = true;
    self.remote.setup_screen = None;
  }

  /// `invalidateRemoteRepositoryData` and `refreshRemoteRepository`.
  fn refresh_remote(&mut self) -> Vec<Effect> {
    self.view.changes_revision += 1;
    vec![Effect::Refresh { revision: true }]
  }

  /// The remote presses; `None` for any other.
  #[expect(
    clippy::string_slice,
    reason = "slices after a checked starts_with() of an ASCII prefix"
  )]
  pub fn remote_press(&mut self, id: &str) -> Option<Vec<Effect>> {
    let cloud = |action| vec![Effect::from(RemoteEffect::Cloud(action))];
    let effects = match id {
      "remote.setup.close" => {
        self.remote.setup = false;
        Vec::new()
      }
      "remote.terminal.close" => {
        self.remote.terminal = false;
        Vec::new()
      }
      "remote.ambiguous.dismiss" => {
        self.remote.ambiguous = None;
        Vec::new()
      }
      // Pending idempotency keys stay: a retry after the refresh must
      // still carry the original key.
      "remote.ambiguous.refresh" => {
        self.remote.ambiguous = None;
        self.refresh_remote()
      }
      // The strip and the connection banner.
      "remote.terminal" => {
        if self.remote.cutoff().is_none() && self.is_remote() {
          self.remote.terminal = true;
        }
        Vec::new()
      }
      "remote.refresh" => {
        if self.remote.cutoff().is_some() {
          return Some(Vec::new());
        }
        let mut effects = self.refresh_remote();
        effects.extend(cloud(CloudAction::Refresh));
        effects
      }
      "remote.wake" => cloud(CloudAction::Wake),
      "remote.reconnect" => cloud(CloudAction::Refresh),
      // The cloud workspace card's buttons, as `CLOUD_ACTIONS` maps them.
      "remote.cloud.provision" => cloud(CloudAction::Provision),
      "remote.cloud.wake" => cloud(CloudAction::Wake),
      "remote.cloud.repair" => cloud(CloudAction::Repair),
      "remote.cloud.delete" => cloud(CloudAction::Delete),
      "remote.cloud.openRepositories" => cloud(CloudAction::OpenRepositories),
      id if id.starts_with("remoteRepo.") => remote(self.remote.form_press(id)),
      // The Overview screen's machines and recent repositories.
      // The cloud row before its workspace runs: the setup dialog, on the
      // cloud workspace's screen.
      "machines.cloud" => {
        self.remote.setup = true;
        self.remote.setup_screen = Some("managed");
        Vec::new()
      }
      // The SSH Hosts section's +: the "Your own VM" form.
      "machines.addSsh" => {
        self.remote.setup = true;
        self.remote.setup_screen = Some("ownVm");
        Vec::new()
      }
      id if id.starts_with("machines.connect:") => {
        let endpoint_id = id["machines.connect:".len()..].to_string();
        vec![Effect::from(RemoteEffect::Connect { endpoint_id })]
      }
      id if id.starts_with("machines.openSaved:") => {
        let id = id["machines.openSaved:".len()..].to_string();
        vec![Effect::from(RemoteEffect::OpenSaved { id })]
      }
      _ => return None,
    };
    Some(effects)
  }

  /// The remote fields; `None` for any other.
  pub fn remote_field(&mut self, field: &str, value: &Value, data: &Data) -> Option<Vec<Effect>> {
    let effects = match field {
      "remote.registerUserManaged" => {
        let values = serde_json::from_value::<UserManagedValues>(value.clone());
        match values {
          Ok(values) if self.remote.setup => {
            self.remote.registering = true;
            vec![Effect::from(RemoteEffect::RegisterUserManaged(values))]
          }
          _ => Vec::new(),
        }
      }
      "remote.terminal.open" => {
        if !self.remote.terminal {
          return Some(Vec::new());
        }
        let label = value["label"]
          .as_str()
          .map(str::to_string)
          // A new session is always fresh: a label already running would
          // attach to it.
          .unwrap_or_else(|| remote::session_label(None, remote::now_ms()));
        let reattach = value["reattach"].as_bool().unwrap_or(false);
        let workspace = self.selected_workspace(data).cloned();
        // The session's name on the host, which its column shows.
        let title = value["title"].as_str().map(str::to_string);
        let effects = self.open_remote_target(workspace.as_ref(), label, title, None, reattach);
        self.remote.terminal = false;
        effects
      }
      "terminal.newRemoteShell" => {
        let dir = value.as_str().unwrap_or_default();
        let workspace = data
          .workspaces(self.repo())
          .iter()
          .find(|w| full_path(w) == dir)
          .cloned();
        let label = remote::session_label(None, remote::now_ms());
        self.open_remote_target(workspace.as_ref(), label, None, None, false)
      }
      field if field.starts_with("remoteRepo.") => remote(self.remote.form_field(field, value)),
      _ => return None,
    };
    Some(effects)
  }

  /// `remoteSessionScope`: where a remote session for `workspace` (the
  /// repository root without one) runs, or why it cannot.
  fn remote_scope(&self, workspace: Option<&Workspace>) -> Result<Value, &'static str> {
    const UNCONNECTED: &str = "Connect to this repository's SSH host to open a terminal.";
    let repository = self.remote.repository.as_ref().ok_or(UNCONNECTED)?;
    let cut_off = self.remote.cutoff().is_some();
    let endpoint = match (&self.remote.endpoint, cut_off) {
      (Some(endpoint), false) => endpoint,
      (_, true) => return Err("Reconnect to the remote host before opening a terminal."),
      (None, false) => return Err(UNCONNECTED),
    };
    let path = workspace.map(full_path);
    let (workspace_id, dir) = remote::terminal_target(
      &repository.canonical_path,
      workspace
        .zip(path.as_deref())
        .map(|(w, p)| (w.workspace_name.as_str(), p)),
    );
    Ok(json!({
      "endpoint": endpoint,
      "repositoryId": repository.canonical_path,
      "workspaceId": workspace_id,
      "remoteWorkingDirectory": dir,
    }))
  }

  /// `terminalPane()?.openRemoteSession`: a persistent session on the host.
  fn open_remote_target(
    &mut self,
    workspace: Option<&Workspace>,
    label: String,
    title: Option<String>,
    agent: Option<RemoteAgentId>,
    reattach: bool,
  ) -> Vec<Effect> {
    match self.remote_scope(workspace) {
      Ok(mut target) => {
        target["label"] = label.into();
        target["title"] = title.into();
        target["launch"] = serde_json::to_value(remote::launch_for(agent)).unwrap_or_default();
        target["reattach"] = reattach.into();
        self.terminal("openRemote", json!({"remote": target}));
        Vec::new()
      }
      Err(why) => vec![toast(
        ToastTone::Warning,
        "Remote terminal unavailable",
        why,
      )],
    }
  }

  /// `startRemoteAgent`: the app's default agent (a repository's setting
  /// lives in a local checkout) in a persistent session on the host.
  pub fn start_remote_agent(&mut self, workspace_id: Option<i64>, data: &Data) -> Vec<Effect> {
    let workspace = workspace_id.and_then(|id| self.workspace_in(self.repo(), id, data));
    // Any other agent the setting names starts Claude.
    let agent = self
      .prefs
      .default_agent
      .as_deref()
      .and_then(remote::remote_agent)
      .unwrap_or(RemoteAgentId::Claude);
    self.set_selected(workspace.as_ref().map(|w| w.id));
    let label = remote::session_label(Some(agent), remote::now_ms());
    self.open_remote_target(workspace.as_ref(), label, None, Some(agent), false)
  }

  /// A remote repository opened: the window shows it as it shows a local
  /// one, its reads and changes going over SSH.
  pub fn remote_opened(&mut self, opened: &Opened) -> Vec<Effect> {
    let mut effects = remote(self.remote.opened(opened));
    self.main_repo = opened.descriptor.canonical_remote_path.clone();
    self.supporting = None;
    self.selected = None;
    self.active_session = None;
    self.multi.clear();
    self.page = Page::Workspace;
    self.view = WorkspaceView::default();
    effects.extend([Effect::ReadPrefs, Effect::Refresh { revision: true }]);
    effects
  }

  /// How a change sent to the remote repository ended
  /// (`useRemoteMutationFeedback().report`).
  pub fn remote_mutation(&mut self, status: &str, reason: Option<&str>) -> Vec<Effect> {
    if status == "ambiguous" {
      self.remote.ambiguous = reason.map(str::to_string);
      // The cached view may miss a change that did land, and the user
      // decides from it whether to retry.
      return self.refresh_remote();
    }
    self.remote.ambiguous = None;
    vec![Effect::Refresh { revision: true }]
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::gpui_shell::remote_repo::tests::repository;

  fn remote_model() -> Model {
    let mut model = Model::new(None);
    let repository = repository();
    model.main_repo = repository.canonical_path.clone();
    model.remote.endpoint = repository.endpoint.clone();
    model.remote.repository = Some(repository);
    model
  }

  #[test]
  fn starts_the_default_agent_on_the_host() {
    let mut model = remote_model();
    model.prefs.default_agent = Some("codex".into());
    assert!(model.start_remote_agent(None, &Data::default()).is_empty());
    let command = model.commands.last().unwrap();
    assert_eq!(command["kind"], "openRemote");
    assert_eq!(command["remote"]["launch"]["agent"], "codex");
    assert!(command["remote"]["label"]
      .as_str()
      .unwrap()
      .starts_with("codex-"));
  }

  #[test]
  fn starts_copilot_on_the_host() {
    let mut model = remote_model();
    model.prefs.default_agent = Some("copilot".into());
    model.start_remote_agent(None, &Data::default());
    let command = &model.commands[0];
    assert_eq!(command["remote"]["launch"]["agent"], "copilot");
    assert!(command["remote"]["label"]
      .as_str()
      .unwrap()
      .starts_with("copilot-"));
  }

  #[test]
  fn a_cut_off_endpoint_opens_no_terminal() {
    let mut model = remote_model();
    let id = model.remote.endpoint.clone().unwrap().id;
    model.remote.cutoffs.insert(id, "key_revoked".into());
    let effects = model.open_remote_target(None, "shell-1".into(), None, None, false);
    assert!(matches!(
      effects.as_slice(),
      [Effect::Toast { description, .. }] if description.starts_with("Reconnect")
    ));
    assert_eq!(model.remote_press("remote.refresh"), Some(Vec::new()));
  }

  #[test]
  fn an_ambiguous_change_refreshes_and_keeps_its_reason() {
    let mut model = remote_model();
    let effects = model.remote_mutation("ambiguous", Some("lost"));
    assert_eq!(model.remote.ambiguous.as_deref(), Some("lost"));
    assert_eq!(effects, [Effect::Refresh { revision: true }]);
    model.remote_mutation("applied", None);
    assert_eq!(model.remote.ambiguous, None);
  }
}
