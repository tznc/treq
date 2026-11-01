//! What the Overview screen lists: the machines Treq opens
//! repositories on (this computer, saved SSH hosts, the Treq Cloud
//! workspace and dev containers), the tools installed on each, and the
//! repositories opened recently on each.
//!
//! The screen itself is `onboarding`; this is its data and what it asks of
//! the backend.

use crate::{
  core::{
    self, devcontainer,
    feature_preview::{self, PreviewFeature},
    pty_remote_supervisor::{PtyBackend, TerminalStatus},
    remote::TreqCommandRequest,
    remote_records, PrerequisiteStatus,
  },
  gpui_shell::{native::Backend, window::remote::RemoteHost},
};

use super::checklist::ToolsHost;

/// Where a machine is.
#[derive(Debug, Clone, PartialEq)]
pub enum Place {
  Local,
  /// A saved SSH host (a user-managed endpoint).
  Ssh {
    endpoint_id: String,
  },
  /// The account's Treq Cloud workspace (a managed VM); its endpoint
  /// is known while it runs. `status` is the instance's, or
  /// [`CLOUD_NOT_CREATED`] before there is one.
  Cloud {
    endpoint_id: Option<String>,
    status: String,
  },
  /// A container Treq started for a local repository.
  DevContainer {
    container_id: String,
    repo_path: String,
    running: bool,
  },
}

impl Place {
  /// Its section in the machine list.
  pub fn section(&self) -> &'static str {
    match self {
      Place::Local => "This Computer",
      Place::Ssh { .. } => "SSH Hosts",
      Place::Cloud { .. } => "Treq Cloud",
      Place::DevContainer { .. } => "Dev Containers",
    }
  }

  /// Treq reaches it over SSH or `docker exec`, so jj must be installed
  /// there too.
  pub fn is_host(&self) -> bool {
    !matches!(self, Place::Local)
  }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Machine {
  /// Stable across reloads: `local`, `ssh:<endpoint>`, `cloud` or
  /// `devcontainer:<container>`.
  pub id: String,
  pub place: Place,
  pub name: String,
  /// The address or state under the name.
  pub detail: String,
  /// What holds the machine's persistent terminals, for a machine Treq
  /// reaches over SSH. `None` where terminals run on this computer.
  pub terminals: Option<PtyBackend>,
}

/// A repository opened recently on a machine.
#[derive(Debug, Clone, PartialEq)]
pub struct Recent {
  pub machine_id: String,
  pub name: String,
  pub path: String,
  /// The remote repository's saved record, which opens it.
  pub saved_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Listing {
  pub machines: Vec<Machine>,
  pub recents: Vec<Recent>,
  /// Not signed in to Treq: no cloud row, a sign-in button instead.
  pub signed_out: bool,
  /// The SSH Hosts section shows, with its + (the Remote SSH preview).
  pub ssh: bool,
  /// The Dev Containers section shows, with its + (the preview).
  pub containers: bool,
}

/// What the screen asks of the backend; faked in tests. Blocking calls run
/// off the main thread.
pub trait MachinesHost: ToolsHost {
  /// Blocks: lists the machines and recent repositories, without the dev
  /// containers, which wait on Docker (see `containers`).
  fn listing(&self) -> Listing;
  /// Blocks: asks Docker for the dev containers, as machines and the
  /// repositories on them.
  fn containers(&self) -> (Vec<Machine>, Vec<Recent>);
  /// Blocks: asks the machine over SSH or `docker exec`.
  fn machine_tools(&self, place: &Place) -> Result<Vec<PrerequisiteStatus>, String>;
  /// Opens the local repository at `path` in this window.
  fn open_local(&self, path: &str);
  /// Forgets the saved SSH host; whether it did.
  fn remove_machine(&self, place: &Place) -> bool;
  /// Blocks: switches an SSH host's terminals to `backend`, first ending
  /// the ones running under its old backend, which would otherwise keep
  /// running unseen.
  fn set_terminal_backend(&self, place: &Place, backend: PtyBackend) -> Result<(), String>;
  /// Blocks: ends every terminal and agent Treq runs on the machine, and
  /// for the daemon stops it.
  fn reset_terminals(&self, place: &Place) -> Result<(), String>;
  /// Blocks: asks the machine what holds its terminals right now
  /// (`treq pty-remote status`).
  fn terminal_status(&self, place: &Place) -> Result<TerminalStatus, String>;
  /// Blocks: asks for a folder; `None` when cancelled.
  fn pick_folder(&self) -> Option<String>;
  /// Blocks: builds and starts Treq's dev container for the repository.
  fn start_dev_container(&self, repo_path: &str) -> Result<(), String>;
}

fn last_segment(path: &str) -> &str {
  path
    .trim_end_matches('/')
    .rsplit('/')
    .find(|part| !part.is_empty())
    .unwrap_or(path)
}

/// The cloud row's status while signed in with no workspace yet.
pub const CLOUD_NOT_CREATED: &str = "not_created";

/// Whether the account can sign in here and has not.
fn signed_out(backend: &Backend) -> bool {
  crate::commands::cloud::owned_cloud(backend.app())
    .is_some_and(|cloud| cloud.snapshot().auth.session.is_none())
}

/// The Treq Cloud row while signed in, so creating the workspace starts
/// from it. Its endpoint is known while the workspace runs.
fn cloud_machine(backend: &Backend) -> Option<Machine> {
  let cloud = crate::commands::cloud::owned_cloud(backend.app())?;
  let snapshot = cloud.snapshot();
  let before = |status: &str, detail: &str| Machine {
    id: "cloud".into(),
    place: Place::Cloud {
      endpoint_id: None,
      status: status.into(),
    },
    name: "Cloud workspace".into(),
    detail: detail.into(),
    terminals: None,
  };
  snapshot.auth.session.as_ref()?;
  let Some(status) = snapshot.cloud.instance_status else {
    return Some(before(CLOUD_NOT_CREATED, "Not created yet"));
  };
  let Some(instance) = status.instance.as_ref().filter(|i| i.status != "deleted") else {
    return Some(before(CLOUD_NOT_CREATED, "Not created yet"));
  };
  let endpoint = status.endpoint_summary();
  // Treq provisions the cloud workspace, so its terminals are the daemon's.
  let terminals = (instance.status == "ready").then_some(PtyBackend::Daemon);
  Some(Machine {
    id: "cloud".into(),
    terminals,
    detail: match &endpoint {
      Some(e) if instance.status == "ready" => format!("{}@{}", e.username, e.hostname),
      _ => instance.status.clone(),
    },
    place: Place::Cloud {
      endpoint_id: endpoint.map(|e| e.id).or(instance.endpoint_id.clone()),
      status: instance.status.clone(),
    },
    name: "Cloud workspace".into(),
  })
}

/// The endpoint of a machine Treq reaches over SSH, for its terminals.
fn remote_endpoint_id(place: &Place) -> Result<&str, String> {
  match place {
    Place::Ssh { endpoint_id } => Ok(endpoint_id),
    Place::Cloud {
      endpoint_id: Some(endpoint_id),
      status,
    } if status == "ready" => Ok(endpoint_id),
    Place::Cloud { .. } => Err("Wake the cloud workspace to see its terminals.".into()),
    Place::Local | Place::DevContainer { .. } => Err("Terminals here run on this computer.".into()),
  }
}

impl MachinesHost for Backend {
  fn listing(&self) -> Listing {
    let (local, endpoints, saved, recent_ids, remote_ssh, containers) = self.with_db(|db| {
      let mut local = remote_records::recent_local_paths(db);
      // Before Treq kept a list, it kept only the last one.
      if local.is_empty() {
        local.extend(db.get_setting("last_opened_repo_path").ok().flatten());
      }
      (
        local,
        remote_records::user_managed_endpoints(db),
        remote_records::all_saved_repositories(db),
        remote_records::recent_repository_ids(db),
        feature_preview::is_enabled(db, PreviewFeature::RemoteSsh),
        feature_preview::is_enabled(db, PreviewFeature::Devcontainers),
      )
    });
    let mut listing = Listing::default();
    listing.machines.push(Machine {
      id: "local".into(),
      place: Place::Local,
      name: "This computer".into(),
      detail: hostname(),
      terminals: None,
    });
    listing.recents.extend(
      local
        .into_iter()
        .filter(|path| std::path::Path::new(path).is_dir())
        .map(|path| Recent {
          machine_id: "local".into(),
          name: last_segment(&path).to_string(),
          path,
          saved_id: None,
        }),
    );
    if remote_ssh {
      listing.machines.extend(endpoints.iter().map(|e| Machine {
        id: format!("ssh:{}", e.id),
        place: Place::Ssh {
          endpoint_id: e.id.clone(),
        },
        name: e.display_name.clone(),
        detail: match e.port {
          22 => format!("{}@{}", e.username, e.hostname),
          port => format!("{}@{}:{port}", e.username, e.hostname),
        },
        terminals: Some(e.terminal_backend),
      }));
    }
    let cloud = cloud_machine(self);
    let cloud_endpoint = cloud.as_ref().and_then(|m| match &m.place {
      Place::Cloud { endpoint_id, .. } => endpoint_id.clone(),
      _ => None,
    });
    listing.machines.extend(cloud);
    listing.signed_out = signed_out(self);
    listing.ssh = remote_ssh;
    listing.containers = containers;
    // Most recently opened first, then the rest in saved order.
    let rank = |id: &str| {
      recent_ids
        .iter()
        .position(|r| r == id)
        .unwrap_or(usize::MAX)
    };
    let mut saved = saved;
    saved.sort_by_key(|r| rank(&r.id));
    listing.recents.extend(saved.into_iter().filter_map(|repo| {
      let machine_id = if endpoints.iter().any(|e| e.id == repo.endpoint_id) {
        remote_ssh.then(|| format!("ssh:{}", repo.endpoint_id))?
      } else if cloud_endpoint.as_deref() == Some(repo.endpoint_id.as_str()) {
        "cloud".to_string()
      } else {
        return None;
      };
      Some(Recent {
        machine_id,
        name: repo.display_name.clone(),
        path: repo.canonical_remote_path.clone(),
        saved_id: Some(repo.id),
      })
    }));
    listing
  }

  fn containers(&self) -> (Vec<Machine>, Vec<Recent>) {
    let (mut machines, mut recents) = (Vec::new(), Vec::new());
    for container in devcontainer::containers() {
      let id = format!("devcontainer:{}", container.id);
      let name = last_segment(&container.repo_path).to_string();
      recents.push(Recent {
        machine_id: id.clone(),
        name: name.clone(),
        path: container.repo_path.clone(),
        saved_id: None,
      });
      machines.push(Machine {
        id,
        detail: if container.running {
          "Running".into()
        } else {
          "Stopped".into()
        },
        place: Place::DevContainer {
          container_id: container.id,
          repo_path: container.repo_path,
          running: container.running,
        },
        name,
        terminals: None,
      });
    }
    (machines, recents)
  }

  fn machine_tools(&self, place: &Place) -> Result<Vec<PrerequisiteStatus>, String> {
    let endpoint_id = match place {
      Place::Local => return Ok(core::check_prerequisites()),
      Place::DevContainer { running: false, .. } => {
        return Err("Start the container to check what is installed in it.".into())
      }
      Place::DevContainer { container_id, .. } => {
        return devcontainer::container_tools(container_id)
      }
      Place::Ssh { endpoint_id } => endpoint_id,
      Place::Cloud {
        endpoint_id: Some(endpoint_id),
        status,
      } if status == "ready" => endpoint_id,
      Place::Cloud { status, .. } if status == CLOUD_NOT_CREATED => {
        return Err("Create the cloud workspace to check what is installed on it.".into())
      }
      Place::Cloud { .. } => {
        return Err("Wake the cloud workspace to check what is installed on it.".into())
      }
    };
    let (endpoint, _) = self
      .machine_endpoint(endpoint_id)
      .ok_or_else(|| "This machine cannot be reached.".to_string())?;
    let value = self.remote_read(
      &endpoint,
      TreqCommandRequest::MachineTools { root: "/".into() },
    )?;
    let mut tools: Vec<PrerequisiteStatus> =
      serde_json::from_value(value).map_err(|e| format!("invalid_remote_json: {e}"))?;
    // It answered, so the treq CLI is installed.
    tools.insert(
      0,
      PrerequisiteStatus {
        binary: "treq".into(),
        installed: true,
      },
    );
    Ok(tools)
  }

  fn open_local(&self, path: &str) {
    let (backend, path) = (self.clone(), path.to_string());
    tauri::async_runtime::spawn_blocking(move || super::onboarding::open_repo_at(&backend, &path));
  }

  fn pick_folder(&self) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    self
      .app()
      .dialog()
      .file()
      .set_title("Select repository")
      .blocking_pick_folder()
      .and_then(|folder| folder.into_path().ok())
      .map(|path| path.to_string_lossy().into_owned())
  }

  fn start_dev_container(&self, repo_path: &str) -> Result<(), String> {
    let claude_code = self.with_db(|db| {
      db.get_repo_setting(repo_path, devcontainer::CLAUDE_CODE_SETTING)
        .ok()
        .flatten()
        .is_none_or(|value| value != "false")
    });
    devcontainer::up(repo_path, claude_code).map(|_| ())
  }

  fn remove_machine(&self, place: &Place) -> bool {
    let Place::Ssh { endpoint_id } = place else {
      return false;
    };
    self.with_db(|db| remote_records::remove_user_managed_endpoint(db, endpoint_id));
    true
  }

  fn set_terminal_backend(&self, place: &Place, backend: PtyBackend) -> Result<(), String> {
    let Place::Ssh { endpoint_id } = place else {
      return Err("Only SSH hosts can switch what holds their terminals.".into());
    };
    let reset = self.reset_terminals(place);
    self.with_db(|db| remote_records::set_terminal_backend(db, endpoint_id, backend));
    // Switched either way: a host that could not be reached has nothing
    // of Treq's to see, but say so.
    reset.map_err(|error| format!("Switched, but the old terminals could not be closed: {error}"))
  }

  fn terminal_status(&self, place: &Place) -> Result<TerminalStatus, String> {
    let (endpoint, _) = self
      .machine_endpoint(remote_endpoint_id(place)?)
      .ok_or_else(|| "This machine cannot be reached.".to_string())?;
    let value = self.remote_read(&endpoint, TreqCommandRequest::PtyStatus)?;
    serde_json::from_value(value).map_err(|e| format!("invalid_remote_json: {e}"))
  }

  fn reset_terminals(&self, place: &Place) -> Result<(), String> {
    let endpoint_id = match place {
      Place::Ssh { endpoint_id } => endpoint_id,
      Place::Cloud {
        endpoint_id: Some(endpoint_id),
        status,
      } if status == "ready" => endpoint_id,
      Place::Cloud { .. } => return Err("Wake the cloud workspace to reset its terminals.".into()),
      Place::Local | Place::DevContainer { .. } => {
        return Err("Terminals here run on this computer.".into())
      }
    };
    let (endpoint, _) = self
      .machine_endpoint(endpoint_id)
      .ok_or_else(|| "This machine cannot be reached.".to_string())?;
    // A reset is safe to send twice, so it goes as a read, which retries
    // once on a dropped connection.
    self
      .remote_read(
        &endpoint,
        TreqCommandRequest::PtyReset {
          backend: endpoint.terminal_backend,
        },
      )
      .map(drop)
  }
}

/// This computer's name: asked once, as it does not change while Treq runs.
fn hostname() -> String {
  static NAME: std::sync::OnceLock<String> = std::sync::OnceLock::new();
  NAME.get_or_init(read_hostname).clone()
}

#[cfg(unix)]
fn read_hostname() -> String {
  let mut name = [0u8; 256];
  // SAFETY: `name` is writable for the length given, and `gethostname`
  // terminates what it writes unless the name is longer, which the last
  // byte below covers.
  let found = unsafe { libc::gethostname(name.as_mut_ptr().cast(), name.len() - 1) } == 0;
  let end = name.iter().position(|b| *b == 0).unwrap_or(name.len() - 1);
  match found {
    true => String::from_utf8_lossy(&name[..end]).trim().to_string(),
    false => String::new(),
  }
}

#[cfg(not(unix))]
fn read_hostname() -> String {
  std::process::Command::new("hostname")
    .output()
    .ok()
    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    .filter(|h| !h.is_empty())
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn names_a_repository_after_its_folder() {
    assert_eq!(last_segment("/home/me/src/app/"), "app");
    assert_eq!(last_segment("~/src/treq"), "treq");
    assert_eq!(last_segment("/"), "/");
  }
}
