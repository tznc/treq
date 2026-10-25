//! A window's remote side, as Dashboard kept it: the remote dialogs (setup,
//! terminal, the unverified-change notice), the SSH endpoint the window is
//! connected to, the repository form under the onboarding card, and the
//! remote repository it shows.
//!
//! [`Remote`] is plain data the model changes; [`RemoteEffect`]s are what
//! the window runs for it, and [`open`] and [`restore_last`] the flows that
//! reach the host, against a [`RemoteHost`] faked in tests.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
  core::{
    remote::{DirectoryListing, RemoteRepoProbe, RepositoryInspection, TreqCommandRequest},
    remote_control_plane::SshEndpoint,
    remote_pty::{PtyLaunchSpec, RemoteAgentId},
    remote_records::{self, RestoreFailure, SavedRepository},
  },
  gpui_shell::{
    remote_repo::{Mutation, RemoteRepository},
    screen::account_data::CloudAction,
  },
};

/// Where browsing starts on a machine: its user's home.
pub const HOME: &str = "~";

/// `REMOTE_MERGE_REASON`: the one action a remote repository lacks
/// (`capabilityNoticeMessages(remoteCapabilities())`).
pub const MERGE_REASON: &str = "Merging a remote workspace is not available yet. Push the branch and merge it from your hosting provider instead.";

/// The "Your own VM" form's confirmed values (`UserManagedFormValues`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct UserManagedValues {
  pub display_name: String,
  pub hostname: String,
  pub port: u16,
  pub username: String,
  pub host_key_fingerprint: String,
  pub auth_identity_reference: String,
  #[serde(default)]
  pub alias: Option<String>,
}

/// `RemoteRepositorySelector`'s state.
#[derive(Debug, Clone, PartialEq)]
pub struct RepoForm {
  pub saved: Vec<SavedRepository>,
  pub selected_id: Option<String>,
  /// The directory browsed to, absolute once listed.
  pub path: String,
  /// What the host listed at `path`.
  pub listing: Option<DirectoryListing>,
  /// Hidden directories show in the listing.
  pub show_hidden: bool,
  /// A listing is on its way.
  pub browsing: bool,
  pub probe: Option<RemoteRepoProbe>,
  pub clone_url: String,
  pub confirm_init: bool,
  pub busy: bool,
  pub error: Option<String>,
  /// `explicitEndpointRepoConnected`: a repository opened on the endpoint.
  pub connected: bool,
  /// `explicitGenerationTransition`.
  pub transition: bool,
}

impl Default for RepoForm {
  fn default() -> Self {
    Self {
      saved: Vec::new(),
      selected_id: None,
      path: HOME.into(),
      listing: None,
      show_hidden: false,
      browsing: false,
      probe: None,
      clone_url: String::new(),
      confirm_init: false,
      busy: false,
      error: None,
      connected: false,
      transition: false,
    }
  }
}

/// The window's remote state.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Remote {
  /// `RemoteSetupDialog` is open.
  pub setup: bool,
  /// The screen it opened on (`managed`, `ownVm`) rather than the choice.
  pub setup_screen: Option<&'static str>,
  /// The "Your own VM" endpoint the form confirmed is being registered.
  pub registering: bool,
  /// `RemoteTerminalDialog` is open.
  pub terminal: bool,
  /// Why the last remote change could not be verified, until dismissed.
  pub ambiguous: Option<String>,
  /// `activeSshEndpoint` and its generation.
  pub endpoint: Option<SshEndpoint>,
  pub generation: u64,
  pub form: RepoForm,
  /// The remote repository on screen (`activeRemoteRepo`).
  pub repository: Option<RemoteRepository>,
  /// Cut-off endpoints and why (`remoteCutoffStore`).
  pub cutoffs: BTreeMap<String, String>,
  /// The cloud workspace's instance state, for the connection banner.
  pub instance_state: Option<String>,
}

/// How the form opens a repository at its path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenHow {
  /// `handleConnectExplicitEndpointRepo`: probe, then open what is there.
  Existing,
  /// `handleCloneExplicitEndpointRepo`, from this URL.
  Clone(String),
  /// `handleInitExplicitEndpointRepo`.
  Init,
}

/// What the window runs for its remote state.
#[derive(Debug, Clone, PartialEq)]
pub enum RemoteEffect {
  /// `refreshSavedRemoteRepos`.
  LoadSaved {
    endpoint_id: String,
    generation: u64,
  },
  /// `handleRegisterUserManaged`.
  RegisterUserManaged(UserManagedValues),
  /// Lists the directories at `path` on the endpoint.
  Browse { endpoint: SshEndpoint, path: String },
  Open {
    endpoint: SshEndpoint,
    generation: u64,
    path: String,
    how: OpenHow,
  },
  /// `handleSelectSavedRemoteRepo`.
  Restore {
    endpoint: SshEndpoint,
    generation: u64,
    descriptor: SavedRepository,
    transition: bool,
  },
  /// The last-opened remote repository, at launch with no local one.
  RestoreLast,
  /// The Overview screen's machine: connect to it and show its
  /// repository form, or the setup dialog if it cannot be reached.
  Connect { endpoint_id: String },
  /// A recent repository on a machine, by its saved id.
  OpenSaved { id: String },
  /// A cloud workspace action ([`crate::core::cloud`]).
  Cloud(CloudAction),
  /// Routes the backend's reads and changes for the repository over SSH
  /// (`remote_repo::set_window`); `None` for a local one.
  Show(Option<RemoteRepository>),
}

/// A repository opened on an endpoint: its record, what inspecting it
/// found, and the endpoint.
#[derive(Debug, Clone, PartialEq)]
pub struct Opened {
  pub descriptor: SavedRepository,
  pub inspection: RepositoryInspection,
  pub endpoint: SshEndpoint,
}

/// `remoteRepoIdentity`: the repository's cache key.
pub fn cache_key(endpoint_id: &str, generation: u64, path: &str) -> String {
  format!("ssh:{endpoint_id}:gen{generation}:{path}")
}

/// `activeRepositoryFromRemote` for a repository just opened.
pub fn repository_of(opened: &Opened) -> RemoteRepository {
  let descriptor = &opened.descriptor;
  let endpoint_id = opened.endpoint.id.clone();
  RemoteRepository {
    cache_key: cache_key(
      &endpoint_id,
      descriptor.endpoint_generation,
      &descriptor.canonical_remote_path,
    ),
    endpoint: Some(opened.endpoint.clone()),
    endpoint_id: Some(endpoint_id),
    canonical_path: descriptor.canonical_remote_path.clone(),
    display_name: descriptor.display_name.clone(),
  }
}

/// `connectionStateFromInstanceState`.
pub fn connection_state(instance: Option<&str>, connected: bool) -> &'static str {
  match instance {
    Some("waking" | "suspended") => "waking",
    Some("degraded" | "failed") => "degraded",
    Some("reprovisioning") => "reconnecting",
    _ if !connected => "offline",
    _ => "online",
  }
}

/// `resolveRemoteTerminalTarget`: the selected workspace's own checkout, or
/// the repository root as workspace `root`. `workspace` is its name and
/// full path.
pub fn terminal_target(repository: &str, workspace: Option<(&str, &str)>) -> (String, String) {
  match workspace {
    Some((name, path)) => (name.to_string(), path.to_string()),
    None => ("root".into(), repository.to_string()),
  }
}

/// `remoteAgentIdFor`: the agents a remote host can start.
pub fn remote_agent(agent: &str) -> Option<RemoteAgentId> {
  match agent {
    "claude" => Some(RemoteAgentId::Claude),
    "codex" => Some(RemoteAgentId::Codex),
    "cursor" => Some(RemoteAgentId::CursorAgent),
    "copilot" => Some(RemoteAgentId::Copilot),
    _ => None,
  }
}

/// `remoteLaunchFor`: the agent binary with no arguments, or a login shell.
pub fn launch_for(agent: Option<RemoteAgentId>) -> PtyLaunchSpec {
  match agent {
    Some(agent) => PtyLaunchSpec::Agent {
      agent,
      args: Vec::new(),
    },
    None => PtyLaunchSpec::Shell,
  }
}

fn base36(mut n: u64) -> String {
  const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
  if n == 0 {
    return "0".into();
  }
  let mut out = Vec::new();
  while n > 0 {
    out.push(DIGITS[(n % 36) as usize]);
    n /= 36;
  }
  out.reverse();
  String::from_utf8(out).unwrap_or_default()
}

/// `newRemoteSessionLabel`: unique per workspace, so a new session never
/// attaches to a running one, and prefixed with its kind.
pub fn session_label(agent: Option<RemoteAgentId>, now_ms: u64) -> String {
  let kind = match agent {
    Some(RemoteAgentId::Claude) => "claude",
    Some(RemoteAgentId::Codex) => "codex",
    Some(RemoteAgentId::CursorAgent) => "cursor_agent",
    Some(RemoteAgentId::Copilot) => "copilot",
    None => "shell",
  };
  format!("{kind}-{}", base36(now_ms))
}

pub fn now_ms() -> u64 {
  chrono::Utc::now().timestamp_millis().max(0) as u64
}

impl Remote {
  /// The endpoint's cutoff reason, if it is cut off.
  pub fn cutoff(&self) -> Option<&str> {
    let id = self
      .repository
      .as_ref()
      .and_then(|r| r.endpoint_id.as_deref())
      .or(self.endpoint.as_ref().map(|e| e.id.as_str()))?;
    self.cutoffs.get(id).map(String::as_str)
  }

  /// `activeEndpointGeneration`, for a repository's descriptor.
  fn set_endpoint(&mut self, endpoint: SshEndpoint, generation: u64) {
    self.endpoint = Some(endpoint);
    self.generation = generation;
  }

  fn load_saved(&self) -> Option<RemoteEffect> {
    let endpoint = self.endpoint.as_ref()?;
    Some(RemoteEffect::LoadSaved {
      endpoint_id: endpoint.id.clone(),
      generation: self.generation,
    })
  }

  /// A connected endpoint shows its repository list
  /// (`adoptManagedConnection` with `openRepositories`, and registering).
  fn list_repositories(&mut self, endpoint: SshEndpoint, generation: u64) -> Vec<RemoteEffect> {
    self.set_endpoint(endpoint, generation);
    self.form.connected = false;
    self.setup = false;
    let path = self.form.path.clone();
    self
      .load_saved()
      .into_iter()
      .chain(self.browse(path))
      .collect()
  }

  /// Lists `path` on the endpoint for the form's directory browser.
  fn browse(&mut self, path: String) -> Option<RemoteEffect> {
    let endpoint = self.endpoint.clone()?;
    self.form.browsing = true;
    Some(RemoteEffect::Browse { endpoint, path })
  }

  /// The host listed a directory: the browser moves into it.
  pub fn browsed(&mut self, endpoint_id: &str, result: Result<DirectoryListing, String>) {
    if self.endpoint.as_ref().map(|e| e.id.as_str()) != Some(endpoint_id) {
      return;
    }
    self.form.browsing = false;
    match result {
      Ok(listing) => {
        self.form.path = listing.path.clone();
        self.form.listing = Some(listing);
        self.form.selected_id = None;
        self.form.probe = None;
        self.form.confirm_init = false;
        self.form.error = None;
      }
      Err(error) => self.form.error = Some(error),
    }
  }

  /// A machine the Overview screen picked is connected: the
  /// form shows for it.
  pub fn connect(&mut self, endpoint: SshEndpoint, generation: u64) -> Vec<RemoteEffect> {
    self.form = RepoForm::default();
    self.list_repositories(endpoint, generation)
  }

  /// The saved repositories of the endpoint, unless it changed meanwhile.
  pub fn saved_listed(&mut self, endpoint_id: &str, generation: u64, saved: Vec<SavedRepository>) {
    if self.endpoint.as_ref().map(|e| e.id.as_str()) == Some(endpoint_id)
      && self.generation == generation
    {
      self.form.saved = saved;
    }
  }

  /// The "Your own VM" endpoint was saved.
  pub fn registered(&mut self, endpoint: SshEndpoint) -> Vec<RemoteEffect> {
    self.registering = false;
    self.list_repositories(endpoint, 0)
  }

  /// `listenManagedEndpoint`: Rust connected to the cloud workspace, or
  /// renewed its certificate.
  pub fn managed_connected(
    &mut self,
    endpoint: SshEndpoint,
    generation: u64,
    open_repositories: bool,
  ) -> Vec<RemoteEffect> {
    let mut effects = self.follow_endpoint(&endpoint);
    if open_repositories {
      effects.extend(self.list_repositories(endpoint, generation));
    } else {
      self.set_endpoint(endpoint, generation);
    }
    effects
  }

  pub fn managed_renewed(&mut self, endpoint: SshEndpoint) -> Vec<RemoteEffect> {
    if self.endpoint.as_ref().map(|e| &e.id) != Some(&endpoint.id) {
      return Vec::new();
    }
    let effects = self.follow_endpoint(&endpoint);
    self.endpoint = Some(endpoint);
    effects
  }

  /// The repository on screen dispatches over the endpoint's new
  /// credentials (`activeRepositoryFromRemote(…, activeSshEndpoint)`).
  fn follow_endpoint(&mut self, endpoint: &SshEndpoint) -> Vec<RemoteEffect> {
    let Some(repository) = self.repository.as_mut() else {
      return Vec::new();
    };
    if repository.endpoint_id.as_deref() != Some(endpoint.id.as_str()) {
      return Vec::new();
    }
    repository.endpoint = Some(endpoint.clone());
    vec![RemoteEffect::Show(Some(repository.clone()))]
  }

  /// A repository opened (`markRemoteRepoOpen`): the form remembers it and
  /// the window shows it.
  pub fn opened(&mut self, opened: &Opened) -> Vec<RemoteEffect> {
    let descriptor = &opened.descriptor;
    self.form.busy = false;
    self.form.error = None;
    self.form.selected_id = Some(descriptor.id.clone());
    self.form.path = descriptor.canonical_remote_path.clone();
    self.form.connected = true;
    self.form.transition = false;
    self.set_endpoint(opened.endpoint.clone(), descriptor.endpoint_generation);
    let repository = repository_of(opened);
    self.repository = Some(repository.clone());
    let mut effects = vec![RemoteEffect::Show(Some(repository))];
    effects.extend(self.load_saved());
    effects
  }

  /// A probe, open, clone or init failed, with what a probe found.
  pub fn failed(&mut self, probe: Option<RemoteRepoProbe>, error: String) {
    self.form.busy = false;
    if probe.is_some() {
      self.form.probe = probe;
    }
    self.form.error = Some(error);
  }

  /// A saved repository did not restore.
  pub fn restore_failed(&mut self, failure: RestoreFailure) {
    self.form.busy = false;
    self.form.connected = false;
    self.form.error = Some(failure.message());
  }

  /// The form's presses (`gpuiRemoteRepo.press`).
  pub fn form_press(&mut self, id: &str) -> Vec<RemoteEffect> {
    let Some(endpoint) = self.endpoint.clone() else {
      return Vec::new();
    };
    let (generation, path) = (self.generation, self.form.path.clone());
    let open = |how| RemoteEffect::Open {
      endpoint: endpoint.clone(),
      generation,
      path: path.clone(),
      how,
    };
    let effect = match id {
      "remoteRepo.reauthenticate" => RemoteEffect::Cloud(CloudAction::Reauthenticate {
        endpoint_id: endpoint.id.clone(),
      }),
      "remoteRepo.up" => {
        let parent = self.form.listing.as_ref().and_then(|l| l.parent.clone());
        return parent
          .and_then(|parent| self.browse(parent))
          .into_iter()
          .collect();
      }
      // A folder that holds no repository: offer to clone or initialize
      // one there, as a probe of it would.
      "remoteRepo.useFolder" => {
        self.form.probe = Some(RemoteRepoProbe {
          host: String::new(),
          path: path.clone(),
          exists: true,
          is_repo: false,
          needs_clone: true,
        });
        self.form.error = None;
        return Vec::new();
      }
      "remoteRepo.open" => open(OpenHow::Existing),
      "remoteRepo.clone" => {
        let url = self.form.clone_url.trim().to_string();
        if url.is_empty() {
          return Vec::new();
        }
        self.form.busy = true;
        open(OpenHow::Clone(url))
      }
      "remoteRepo.init" => {
        if !self.form.confirm_init {
          return Vec::new();
        }
        self.form.busy = true;
        open(OpenHow::Init)
      }
      _ => return Vec::new(),
    };
    if id != "remoteRepo.reauthenticate" {
      self.form.error = None;
    }
    vec![effect]
  }

  /// The form's fields (`gpuiRemoteRepo.field`).
  pub fn form_field(&mut self, field: &str, value: &Value) -> Vec<RemoteEffect> {
    match field {
      "remoteRepo.select" => {
        let (Some(endpoint), Some(id)) = (self.endpoint.clone(), value.as_str()) else {
          return Vec::new();
        };
        let Some(descriptor) = self.form.saved.iter().find(|r| r.id == id).cloned() else {
          return Vec::new();
        };
        self.form.selected_id = Some(descriptor.id.clone());
        self.form.path = descriptor.canonical_remote_path.clone();
        self.form.error = None;
        self.form.busy = true;
        return vec![RemoteEffect::Restore {
          endpoint,
          generation: self.generation,
          descriptor,
          transition: self.form.transition,
        }];
      }
      "remoteRepo.browse" => {
        let Some(path) = value.as_str().filter(|p| !p.is_empty()) else {
          return Vec::new();
        };
        return self.browse(path.to_string()).into_iter().collect();
      }
      "remoteRepo.showHidden" => self.form.show_hidden = value.as_bool().unwrap_or(false),
      "remoteRepo.cloneUrl" => self.form.clone_url = value.as_str().unwrap_or_default().into(),
      "remoteRepo.confirmInit" => self.form.confirm_init = value.as_bool().unwrap_or(false),
      _ => {}
    }
    Vec::new()
  }

  /// The form under the onboarding card, while an endpoint is connected
  /// and no repository shows (`gpuiRemoteRepo.form`).
  pub fn form_json(&self) -> Option<Value> {
    let endpoint = self.endpoint.as_ref()?;
    if self.repository.is_some() {
      return None;
    }
    let form = &self.form;
    let cutoff = self.cutoffs.get(&endpoint.id);
    Some(json!({
      "endpointId": endpoint.id,
      "cutoff": cutoff,
      "selector": cutoff.is_none() && !form.connected,
      "saved": form.saved.iter().map(|r| json!({
        "id": r.id,
        "display_name": r.display_name,
        "canonical_remote_path": r.canonical_remote_path,
      })).collect::<Vec<_>>(),
      "selectedId": form.selected_id,
      "path": form.path,
      "listing": form.listing,
      "showHidden": form.show_hidden,
      "browsing": form.browsing,
      "probe": form.probe.as_ref().map(|p| json!({"is_repo": p.is_repo, "needs_clone": p.needs_clone})),
      "cloneUrl": form.clone_url,
      "confirmInit": form.confirm_init,
      "busy": form.busy,
      "error": form.error,
    }))
  }
}

/// What the remote flows ask of the backend; blocks. Part of the window's
/// host.
pub trait RemoteHost: Send + Sync {
  fn saved_repositories(&self, endpoint_id: &str, generation: u64) -> Vec<SavedRepository>;
  /// Saves the endpoint the form confirmed and returns it.
  fn register_user_managed(&self, values: &UserManagedValues) -> SshEndpoint;
  /// `dispatchOverSsh`: a typed read on `endpoint`.
  fn remote_read(
    &self,
    endpoint: &SshEndpoint,
    request: TreqCommandRequest,
  ) -> Result<Value, String>;
  /// `dispatchKeyedMutationOverSsh`: a change, keyed per endpoint
  /// generation so a retry never runs it twice.
  fn remote_change(
    &self,
    endpoint: &SshEndpoint,
    request: TreqCommandRequest,
  ) -> Result<Mutation, String>;
  /// Records the repository as trusted just now, and as the last opened.
  fn save_remote_repository(
    &self,
    endpoint_id: &str,
    generation: u64,
    path: &str,
  ) -> Result<SavedRepository, String>;
  /// `restoreSavedRemoteRepository` on `endpoint`.
  fn restore_remote(
    &self,
    descriptor: &SavedRepository,
    endpoint: &SshEndpoint,
    generation: u64,
    transition: bool,
  ) -> Result<(SavedRepository, RepositoryInspection), RestoreFailure>;
  /// The last-opened remote repository and the endpoint and generation it
  /// would reconnect on, when there is one to try.
  fn last_remote(&self) -> Option<(SavedRepository, SshEndpoint, u64)>;
  /// The saved repository `id` and the endpoint and generation it would
  /// reconnect on, while that endpoint can be reached.
  fn saved_remote(&self, id: &str) -> Option<(SavedRepository, SshEndpoint, u64)>;
  /// The endpoint `id` names (a saved SSH host, or the cloud workspace
  /// while it runs) and its generation.
  fn machine_endpoint(&self, id: &str) -> Option<(SshEndpoint, u64)>;
  /// Starts a cloud workspace action; it reports through the account.
  fn cloud_action(&self, action: CloudAction);
  /// `remote_repo::set_window` for this window.
  fn show_remote(&self, repository: Option<RemoteRepository>);
}

fn parse<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, String> {
  serde_json::from_value(value).map_err(|e| format!("invalid_remote_json: {e}"))
}

/// `inspectAndRegisterPath`.
fn inspect_and_register(
  host: &dyn RemoteHost,
  endpoint: &SshEndpoint,
  generation: u64,
  path: &str,
) -> Result<Opened, String> {
  let canonical = remote_records::canonicalize_remote_path(path);
  let inspection: RepositoryInspection = parse(host.remote_read(
    endpoint,
    TreqCommandRequest::InspectRepository {
      repo: canonical.clone(),
    },
  )?)?;
  let descriptor = host.save_remote_repository(&endpoint.id, generation, &canonical)?;
  Ok(Opened {
    descriptor,
    inspection,
    endpoint: endpoint.clone(),
  })
}

/// The directories at `path` on `endpoint` (`~` is its home).
pub fn list_directories(
  host: &dyn RemoteHost,
  endpoint: &SshEndpoint,
  path: &str,
) -> Result<DirectoryListing, String> {
  host
    .remote_read(
      endpoint,
      TreqCommandRequest::ListDirectories {
        path: path.to_string(),
      },
    )
    .and_then(parse)
}

/// `handleProbeExplicitEndpointRepo`: what is at `path` on `endpoint`.
pub fn probe(
  host: &dyn RemoteHost,
  endpoint: &SshEndpoint,
  path: &str,
) -> Result<RemoteRepoProbe, String> {
  host
    .remote_read(
      endpoint,
      TreqCommandRequest::ProbeRepo {
        repo: path.to_string(),
      },
    )
    .and_then(parse)
}

/// How a form flow ended: the repository it opened, or why not and what a
/// probe found on the way.
pub type OpenResult = Result<Opened, (Option<RemoteRepoProbe>, String)>;

/// Opens the repository at `path` on `endpoint` as `how` says.
pub fn open(
  host: &dyn RemoteHost,
  endpoint: &SshEndpoint,
  generation: u64,
  path: &str,
  how: &OpenHow,
) -> OpenResult {
  let canonical = remote_records::canonicalize_remote_path(path);
  let change = |request| match host.remote_change(endpoint, request) {
    Ok(Mutation::Ambiguous(reason)) => Err((None, reason)),
    Ok(_) => Ok(()),
    Err(error) => Err((None, error)),
  };
  match how {
    OpenHow::Existing => {
      let probe = probe(host, endpoint, path).map_err(|e| (None, e))?;
      if !probe.is_repo {
        let message = if probe.needs_clone {
          "No repository at that path. Clone or initialize it."
        } else {
          "No repository found at that path on the remote machine."
        };
        return Err((Some(probe), message.into()));
      }
      // A plain Git repository needs jj colocated before its workspaces and
      // commits load; one that has it is left as it is.
      change(TreqCommandRequest::InitRepo {
        repo: canonical,
        idempotency_key: String::new(),
      })?;
    }
    OpenHow::Clone(url) => change(TreqCommandRequest::CloneRepo {
      repo_url: url.clone(),
      destination: canonical,
      idempotency_key: String::new(),
    })?,
    OpenHow::Init => change(TreqCommandRequest::InitRepo {
      repo: canonical,
      idempotency_key: String::new(),
    })?,
  }
  inspect_and_register(host, endpoint, generation, path).map_err(|e| (None, e))
}

/// The saved repository `id`, restored on its endpoint; `Err(None)` when
/// the endpoint cannot be reached.
pub fn open_saved(host: &dyn RemoteHost, id: &str) -> Result<Opened, Option<RestoreFailure>> {
  let (descriptor, endpoint, generation) = host.saved_remote(id).ok_or(None)?;
  let (descriptor, inspection) = host
    .restore_remote(&descriptor, &endpoint, generation, false)
    .map_err(Some)?;
  Ok(Opened {
    descriptor,
    inspection,
    endpoint,
  })
}

/// The last-opened remote repository, restored only once its endpoint
/// reconnects and is trusted again; `None` leaves the window on
/// onboarding.
pub fn restore_last(host: &dyn RemoteHost) -> Option<Opened> {
  let (descriptor, endpoint, generation) = host.last_remote()?;
  let (descriptor, inspection) = host
    .restore_remote(&descriptor, &endpoint, generation, false)
    .ok()?;
  Some(Opened {
    descriptor,
    inspection,
    endpoint,
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn opens_the_terminal_in_the_selected_workspaces_checkout() {
    assert_eq!(
      terminal_target("/srv/app", Some(("feat", "/srv/app/.treq/workspaces/feat"))),
      ("feat".into(), "/srv/app/.treq/workspaces/feat".into())
    );
  }

  #[test]
  fn falls_back_to_the_repository_root_without_a_workspace() {
    assert_eq!(
      terminal_target("/srv/app", None),
      ("root".into(), "/srv/app".into())
    );
  }

  #[test]
  fn maps_sidebar_agents_onto_the_remote_allow_list() {
    assert_eq!(remote_agent("claude"), Some(RemoteAgentId::Claude));
    assert_eq!(remote_agent("codex"), Some(RemoteAgentId::Codex));
    assert_eq!(remote_agent("cursor"), Some(RemoteAgentId::CursorAgent));
    assert_eq!(remote_agent("copilot"), Some(RemoteAgentId::Copilot));
    assert_eq!(remote_agent("aider"), None);
  }

  #[test]
  fn launches_the_agent_binary_as_a_typed_spec_or_a_login_shell() {
    assert_eq!(
      launch_for(Some(RemoteAgentId::Codex)),
      PtyLaunchSpec::Agent {
        agent: RemoteAgentId::Codex,
        args: Vec::new()
      }
    );
    assert_eq!(launch_for(None), PtyLaunchSpec::Shell);
  }

  #[test]
  fn builds_distinct_labels_prefixed_with_the_session_kind() {
    let a = session_label(None, 1_700_000_000_000);
    let b = session_label(None, 1_700_000_000_001);
    assert!(a.starts_with("shell-"));
    assert_ne!(a, b);
    assert!(session_label(Some(RemoteAgentId::CursorAgent), 1).starts_with("cursor_agent-"));
    assert!(session_label(Some(RemoteAgentId::Copilot), 1).starts_with("copilot-"));
    assert_eq!(base36(35), "z");
    assert_eq!(base36(36), "10");
  }

  #[test]
  fn reads_the_connection_state_from_the_instance() {
    assert_eq!(connection_state(Some("suspended"), true), "waking");
    assert_eq!(connection_state(Some("failed"), true), "degraded");
    assert_eq!(
      connection_state(Some("reprovisioning"), true),
      "reconnecting"
    );
    assert_eq!(connection_state(Some("ready"), false), "offline");
    assert_eq!(connection_state(None, true), "online");
  }

  #[test]
  fn keys_a_repository_by_endpoint_generation_and_path() {
    assert_eq!(cache_key("ep", 2, "~/a"), "ssh:ep:gen2:~/a");
  }
}
