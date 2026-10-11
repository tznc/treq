//! Records kept in the settings database, in the same formats the desktop
//! frontend uses (`remote-endpoints.ts`, `remote-repository.ts`), plus this
//! app's restore snapshot.

use serde::{Deserialize, Serialize};
use treq_lib::core::remote_control_plane::{
  SshAuthentication, SshEndpoint, SshEndpointSource, SshTransport, TrustedHostKey,
};

use crate::backend;

const USER_MANAGED_ENDPOINTS: &str = "remote_user_managed_endpoints";
const SAVED_REPOSITORIES: &str = "remote_saved_repositories";
const SESSION: &str = "gpui_mobile_session";

/// A user-managed SSH host the user trusted on this device.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserManagedEndpoint {
  pub id: String,
  pub display_name: String,
  pub hostname: String,
  pub port: u16,
  pub username: String,
  pub host_key_fingerprint: String,
  pub auth_identity_reference: String,
  #[serde(default)]
  pub alias: Option<String>,
  pub created_at: String,
}

impl UserManagedEndpoint {
  pub fn to_endpoint(&self) -> SshEndpoint {
    SshEndpoint {
      id: self.id.clone(),
      instance_id: None,
      source: SshEndpointSource::UserManaged,
      hostname: self.hostname.clone(),
      port: self.port,
      username: self.username.clone(),
      host_keys: vec![TrustedHostKey {
        algorithm: "unknown".into(),
        fingerprint_sha256: self.host_key_fingerprint.clone(),
        comment: None,
      }],
      authentication: SshAuthentication::PublicKey {
        key_reference: self.auth_identity_reference.clone(),
      },
      transport: SshTransport::Direct,
    }
  }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedRepository {
  pub id: String,
  pub endpoint_id: String,
  pub endpoint_generation: u64,
  #[serde(alias = "remote_path")]
  pub canonical_remote_path: String,
  pub display_name: String,
  #[serde(default)]
  pub last_successful_trust_validation: Option<String>,
}

/// Which endpoint the restore snapshot reconnects to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SessionEndpoint {
  Managed,
  UserManaged { id: String },
}

/// Where the app was, so a relaunch can reopen it. Screens only name remote
/// objects; every screen refetches its data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionSnapshot {
  pub endpoint: SessionEndpoint,
  #[serde(default)]
  pub repo_path: Option<String>,
  #[serde(default)]
  pub workspace: Option<SnapshotWorkspace>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SnapshotWorkspace {
  pub id: String,
  pub name: String,
}

fn read_list<T: for<'de> Deserialize<'de>>(key: &str) -> Vec<T> {
  backend::get()
    .setting(key)
    .and_then(|raw| serde_json::from_str::<Vec<serde_json::Value>>(&raw).ok())
    .unwrap_or_default()
    .into_iter()
    .filter_map(|value| serde_json::from_value(value).ok())
    .collect()
}

fn write_list<T: Serialize>(key: &str, items: &[T]) {
  match serde_json::to_string(items) {
    Ok(json) => backend::get().set_setting(key, &json),
    Err(error) => log::error!("serializing {key}: {error}"),
  }
}

pub fn user_managed_endpoints() -> Vec<UserManagedEndpoint> {
  read_list(USER_MANAGED_ENDPOINTS)
}

/// Saves `endpoint` first in the list, replacing any record with its id.
pub fn save_user_managed_endpoint(endpoint: UserManagedEndpoint) {
  let mut list = user_managed_endpoints();
  list.retain(|e| e.id != endpoint.id);
  list.insert(0, endpoint);
  write_list(USER_MANAGED_ENDPOINTS, &list);
}

pub fn remove_user_managed_endpoint(id: &str) {
  let mut list = user_managed_endpoints();
  list.retain(|e| e.id != id);
  write_list(USER_MANAGED_ENDPOINTS, &list);
}

/// Normalizes a remote path: trims, keeps a leading `~`, turns `\` into `/`,
/// drops empty and `.` segments and resolves `..` without leaving the root.
pub fn canonicalize_remote_path(path: &str) -> String {
  let path = path.trim().replace('\\', "/");
  let (prefix, rest) = if let Some(rest) = path.strip_prefix('~') {
    ("~/", rest)
  } else if let Some(rest) = path.strip_prefix('/') {
    ("/", rest)
  } else {
    ("", path.as_str())
  };
  let mut segments: Vec<&str> = Vec::new();
  for segment in rest.split('/') {
    match segment {
      "" | "." => {}
      ".." => {
        segments.pop();
      }
      segment => segments.push(segment),
    }
  }
  let joined = segments.join("/");
  match prefix {
    "~/" if joined.is_empty() => "~".into(),
    "/" if joined.is_empty() => "/".into(),
    prefix => format!("{prefix}{joined}"),
  }
}

pub fn repository_id(endpoint_id: &str, generation: u64, canonical: &str) -> String {
  format!("remote-repo:{endpoint_id}:gen{generation}:{canonical}")
}

pub fn saved_repositories(endpoint_id: &str, generation: u64) -> Vec<SavedRepository> {
  read_list::<SavedRepository>(SAVED_REPOSITORIES)
    .into_iter()
    .filter(|r| r.endpoint_id == endpoint_id && r.endpoint_generation == generation)
    .collect()
}

/// Records a repository that probed successfully, most recent first.
pub fn upsert_saved_repository(endpoint_id: &str, generation: u64, path: &str) {
  let canonical = canonicalize_remote_path(path);
  let mut list: Vec<SavedRepository> = read_list(SAVED_REPOSITORIES);
  let existing = list.iter().position(|r| {
    r.endpoint_id == endpoint_id
      && r.endpoint_generation == generation
      && r.canonical_remote_path == canonical
  });
  let record = match existing {
    Some(index) => list.remove(index),
    None => SavedRepository {
      id: repository_id(endpoint_id, generation, &canonical),
      endpoint_id: endpoint_id.into(),
      endpoint_generation: generation,
      display_name: canonical.clone(),
      canonical_remote_path: canonical,
      last_successful_trust_validation: None,
    },
  };
  list.insert(0, record);
  write_list(SAVED_REPOSITORIES, &list);
}

/// The generation repositories are keyed by: managed endpoints carry it,
/// user-managed ones are always 0.
pub fn endpoint_generation(endpoint: &SshEndpoint) -> u64 {
  match endpoint.source {
    SshEndpointSource::Managed { generation, .. } => generation,
    _ => 0,
  }
}

pub fn session_snapshot() -> Option<SessionSnapshot> {
  serde_json::from_str(&backend::get().setting(SESSION)?).ok()
}

pub fn save_session_snapshot(snapshot: Option<&SessionSnapshot>) {
  let json = snapshot
    .and_then(|s| serde_json::to_string(s).ok())
    .unwrap_or_default();
  backend::get().set_setting(SESSION, &json);
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn canonicalizes_paths_like_the_desktop_client() {
    assert_eq!(canonicalize_remote_path("  ~/src//treq/ "), "~/src/treq");
    assert_eq!(canonicalize_remote_path("/srv/./a/../b"), "/srv/b");
    assert_eq!(canonicalize_remote_path("/../.."), "/");
    assert_eq!(canonicalize_remote_path("~"), "~");
    assert_eq!(canonicalize_remote_path("a\\b"), "a/b");
  }

  #[test]
  fn reads_the_legacy_remote_path_field() {
    let record: SavedRepository = serde_json::from_str(
            r#"{"id":"x","endpoint_id":"e","endpoint_generation":0,"remote_path":"/r","display_name":"/r"}"#,
        )
        .unwrap();
    assert_eq!(record.canonical_remote_path, "/r");
  }

  #[test]
  fn user_managed_endpoints_pin_the_host_key() {
    let endpoint = UserManagedEndpoint {
      id: "user-managed-1".into(),
      display_name: "box".into(),
      hostname: "box.local".into(),
      port: 22,
      username: "me".into(),
      host_key_fingerprint: "SHA256:abc".into(),
      auth_identity_reference: "keystore:device".into(),
      alias: None,
      created_at: "2026-01-01T00:00:00Z".into(),
    }
    .to_endpoint();
    assert_eq!(endpoint.host_keys[0].fingerprint_sha256, "SHA256:abc");
    assert_eq!(endpoint.source, SshEndpointSource::UserManaged);
  }

  #[test]
  fn snapshot_round_trips() {
    let snapshot = SessionSnapshot {
      endpoint: SessionEndpoint::UserManaged { id: "u1".into() },
      repo_path: Some("~/r".into()),
      workspace: Some(SnapshotWorkspace {
        id: "3".into(),
        name: "feature".into(),
      }),
    };
    let json = serde_json::to_string(&snapshot).unwrap();
    assert!(json.contains(r#""kind":"user_managed""#));
    assert_eq!(
      serde_json::from_str::<SessionSnapshot>(&json).unwrap(),
      snapshot
    );
  }
}
