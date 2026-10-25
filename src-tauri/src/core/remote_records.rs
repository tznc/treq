//! Remote records kept in the settings database, in the formats the
//! frontend uses (`remote-endpoints.ts`, `remote-repository.ts`), plus the
//! mobile app's restore snapshot. Desktop and mobile share them.

use serde::{Deserialize, Serialize};

use super::{
  remote::RepositoryInspection,
  remote_control_plane::{
    SshAuthentication, SshEndpoint, SshEndpointSource, SshTransport, TrustedHostKey,
  },
};
use crate::db::Database;

const USER_MANAGED_ENDPOINTS: &str = "remote_user_managed_endpoints";
const SAVED_REPOSITORIES: &str = "remote_saved_repositories";
const SESSION: &str = "gpui_mobile_session";

fn setting(db: &Database, key: &str) -> Option<String> {
  db.get_setting(key).ok().flatten().filter(|v| !v.is_empty())
}

fn set_setting(db: &Database, key: &str, value: &str) {
  if let Err(error) = db.set_setting(key, value) {
    log::error!("saving setting {key}: {error}");
  }
}

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
  /// What holds the machine's persistent terminals. Records saved before
  /// the setting existed read as the default, the Treq daemon.
  #[serde(
    default,
    skip_serializing_if = "crate::core::pty_remote_supervisor::PtyBackend::is_default"
  )]
  pub terminal_backend: crate::core::pty_remote_supervisor::PtyBackend,
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
      terminal_backend: self.terminal_backend,
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

fn read_list<T: for<'de> Deserialize<'de>>(db: &Database, key: &str) -> Vec<T> {
  setting(db, key)
    .and_then(|raw| serde_json::from_str::<Vec<serde_json::Value>>(&raw).ok())
    .unwrap_or_default()
    .into_iter()
    .filter_map(|value| serde_json::from_value(value).ok())
    .collect()
}

fn write_list<T: Serialize>(db: &Database, key: &str, items: &[T]) {
  match serde_json::to_string(items) {
    Ok(json) => set_setting(db, key, &json),
    Err(error) => log::error!("serializing {key}: {error}"),
  }
}

pub fn user_managed_endpoints(db: &Database) -> Vec<UserManagedEndpoint> {
  read_list(db, USER_MANAGED_ENDPOINTS)
}

/// Saves `endpoint` first in the list, replacing any record with its id.
pub fn save_user_managed_endpoint(db: &Database, endpoint: UserManagedEndpoint) {
  let mut list = user_managed_endpoints(db);
  list.retain(|e| e.id != endpoint.id);
  list.insert(0, endpoint);
  write_list(db, USER_MANAGED_ENDPOINTS, &list);
}

/// Sets which program holds `id`'s terminals. `false` when no saved machine
/// has that id.
pub fn set_terminal_backend(
  db: &Database,
  id: &str,
  backend: crate::core::pty_remote_supervisor::PtyBackend,
) -> bool {
  let mut list = user_managed_endpoints(db);
  let Some(endpoint) = list.iter_mut().find(|e| e.id == id) else {
    return false;
  };
  endpoint.terminal_backend = backend;
  write_list(db, USER_MANAGED_ENDPOINTS, &list);
  true
}

pub fn remove_user_managed_endpoint(db: &Database, id: &str) {
  let mut list = user_managed_endpoints(db);
  list.retain(|e| e.id != id);
  write_list(db, USER_MANAGED_ENDPOINTS, &list);
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

pub fn saved_repositories(
  db: &Database,
  endpoint_id: &str,
  generation: u64,
) -> Vec<SavedRepository> {
  read_list::<SavedRepository>(db, SAVED_REPOSITORIES)
    .into_iter()
    .filter(|r| r.endpoint_id == endpoint_id && r.endpoint_generation == generation)
    .collect()
}

/// Records a repository that probed successfully, most recent first.
pub fn upsert_saved_repository(db: &Database, endpoint_id: &str, generation: u64, path: &str) {
  if let Err(error) = upsert(db, endpoint_id, generation, path, None, None) {
    log::warn!("saving remote repository {path}: {error}");
  }
}

/// Every saved repository, most recent first.
pub fn all_saved_repositories(db: &Database) -> Vec<SavedRepository> {
  read_list(db, SAVED_REPOSITORIES)
}

/// `getSavedRemoteRepository`.
pub fn saved_repository(db: &Database, id: &str) -> Option<SavedRepository> {
  all_saved_repositories(db).into_iter().find(|r| r.id == id)
}

/// What a descriptor must never carry (`DESCRIPTOR_SECRET_KEYS`).
const SECRET_WORDS: &[&str] = &[
  "private_key",
  "privatekey",
  "password",
  "passphrase",
  "credential",
  "secret",
  "ssh_key",
  "sshkey",
];

/// `isSecretFreeDescriptor`: nothing in the record looks like a credential.
pub fn is_secret_free(record: &SavedRepository) -> bool {
  let json = serde_json::to_string(record)
    .unwrap_or_default()
    .to_lowercase();
  !SECRET_WORDS.iter().any(|word| json.contains(word))
}

/// `upsertSavedRemoteRepository`: saves `path` on the endpoint generation,
/// first in the list. An existing record for the same path keeps its id,
/// its name unless `display_name` is given, and its last trust validation
/// unless `trust` is given.
pub fn upsert(
  db: &Database,
  endpoint_id: &str,
  generation: u64,
  path: &str,
  display_name: Option<&str>,
  trust: Option<Option<String>>,
) -> Result<SavedRepository, String> {
  let canonical = canonicalize_remote_path(path);
  if canonical.is_empty() {
    return Err("Remote path is required".into());
  }
  let mut list = all_saved_repositories(db);
  let existing = list
    .iter()
    .position(|r| {
      r.endpoint_id == endpoint_id
        && r.endpoint_generation == generation
        && canonicalize_remote_path(&r.canonical_remote_path) == canonical
    })
    .map(|index| list.remove(index));
  let name = display_name
    .map(str::trim)
    .filter(|n| !n.is_empty())
    .map(str::to_string)
    .or_else(|| existing.as_ref().map(|r| r.display_name.clone()))
    .unwrap_or_else(|| canonical.clone());
  let record = SavedRepository {
    id: existing
      .as_ref()
      .map(|r| r.id.clone())
      .unwrap_or_else(|| repository_id(endpoint_id, generation, &canonical)),
    endpoint_id: endpoint_id.into(),
    endpoint_generation: generation,
    canonical_remote_path: canonical,
    display_name: name,
    last_successful_trust_validation: trust
      .unwrap_or_else(|| existing.and_then(|r| r.last_successful_trust_validation)),
  };
  if !is_secret_free(&record) {
    return Err("Remote repository descriptors must not contain secrets".into());
  }
  list.insert(0, record.clone());
  write_list(db, SAVED_REPOSITORIES, &list);
  Ok(record)
}

/// `invalidateTrustAfterGenerationChange`: the endpoint moved to a new
/// generation, so what was trusted on `previous` is no longer.
pub fn invalidate_trust_after_generation_change(db: &Database, endpoint_id: &str, previous: u64) {
  let mut list = all_saved_repositories(db);
  for record in &mut list {
    if record.endpoint_id == endpoint_id && record.endpoint_generation == previous {
      record.last_successful_trust_validation = None;
    }
  }
  write_list(db, SAVED_REPOSITORIES, &list);
}

/// The remote repository opened last, separate from the local
/// `last_opened_repo_path`.
pub const LAST_OPENED_REMOTE_REPOSITORY: &str = "last_opened_remote_repo_id";
const RECENT_REMOTE_REPOSITORIES: &str = "remote_recent_repository_ids";
/// The legacy blob, never restored from without the trust sequence.
const LEGACY_LAST_OPENED_REMOTE: &str = "last_opened_remote_repo";

/// `rememberLastOpenedRemoteRepository`: also first of the ten recents.
pub fn remember_last_opened(db: &Database, id: &str) {
  set_setting(db, LAST_OPENED_REMOTE_REPOSITORY, id);
  let mut recent = recent_repository_ids(db);
  recent.retain(|r| r != id);
  recent.insert(0, id.to_string());
  recent.truncate(10);
  set_setting(
    db,
    RECENT_REMOTE_REPOSITORIES,
    &serde_json::to_string(&recent).unwrap_or_default(),
  );
}

pub fn last_opened_id(db: &Database) -> Option<String> {
  setting(db, LAST_OPENED_REMOTE_REPOSITORY)
}

pub fn recent_repository_ids(db: &Database) -> Vec<String> {
  setting(db, RECENT_REMOTE_REPOSITORIES)
    .and_then(|raw| serde_json::from_str(&raw).ok())
    .unwrap_or_default()
}

/// Local repositories opened most recently first, for the Open a
/// Repository screen.
const RECENT_LOCAL_REPOSITORIES: &str = "recent_local_repository_paths";

pub fn remember_local_opened(db: &Database, path: &str) {
  let mut recent = recent_local_paths(db);
  recent.retain(|p| p != path);
  recent.insert(0, path.to_string());
  recent.truncate(10);
  set_setting(
    db,
    RECENT_LOCAL_REPOSITORIES,
    &serde_json::to_string(&recent).unwrap_or_default(),
  );
}

pub fn recent_local_paths(db: &Database) -> Vec<String> {
  setting(db, RECENT_LOCAL_REPOSITORIES)
    .and_then(|raw| serde_json::from_str(&raw).ok())
    .unwrap_or_default()
}

/// `clearLastOpenedRemoteRepository`.
pub fn clear_last_opened(db: &Database) {
  set_setting(db, LAST_OPENED_REMOTE_REPOSITORY, "");
  set_setting(db, LEGACY_LAST_OPENED_REMOTE, "");
}

/// Why a saved repository was not restored (`RestoreFailureReason`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreFailure {
  ReconnectFailed,
  HostKeyMismatch,
  GenerationMismatch,
  InspectFailed,
}

impl RestoreFailure {
  /// The message the repository form shows.
  pub fn message(self) -> String {
    match self {
      RestoreFailure::GenerationMismatch => {
        "Endpoint generation changed. Confirm a trust transition before restoring.".into()
      }
      RestoreFailure::ReconnectFailed => "Restore refused: reconnect failed.".into(),
      RestoreFailure::HostKeyMismatch => "Restore refused: host key mismatch.".into(),
      RestoreFailure::InspectFailed => "Restore refused: inspect failed.".into(),
    }
  }
}

/// The steps a restore runs on the endpoint, in order.
pub trait RestoreSteps {
  fn reconnect(&self) -> bool;
  fn validate_host_key(&self) -> bool;
  fn inspect(&self, canonical_path: &str) -> Result<RepositoryInspection, String>;
}

/// `restoreSavedRemoteRepository`: a saved repository opens only after a
/// reconnect, the host key, the endpoint generation (or a transition the
/// user confirmed) and a typed inspect all succeed. A failure leaves it
/// closed. `db` is taken for each write, never across the SSH steps.
pub fn restore_saved_repository<D: std::ops::Deref<Target = Database>>(
  db: impl Fn() -> D,
  descriptor: &SavedRepository,
  current_generation: u64,
  transition: bool,
  steps: &dyn RestoreSteps,
  now_iso: &str,
) -> Result<(SavedRepository, RepositoryInspection), RestoreFailure> {
  if !steps.reconnect() {
    return Err(RestoreFailure::ReconnectFailed);
  }
  if !steps.validate_host_key() {
    return Err(RestoreFailure::HostKeyMismatch);
  }
  if descriptor.endpoint_generation != current_generation && !transition {
    invalidate_trust_after_generation_change(
      &db(),
      &descriptor.endpoint_id,
      descriptor.endpoint_generation,
    );
    return Err(RestoreFailure::GenerationMismatch);
  }
  let inspection = steps
    .inspect(&descriptor.canonical_remote_path)
    .map_err(|_| RestoreFailure::InspectFailed)?;
  let generation = if transition {
    current_generation
  } else {
    descriptor.endpoint_generation
  };
  let db = db();
  let record = upsert(
    &db,
    &descriptor.endpoint_id,
    generation,
    &descriptor.canonical_remote_path,
    Some(&descriptor.display_name),
    Some(Some(now_iso.to_string())),
  )
  .map_err(|_| RestoreFailure::InspectFailed)?;
  remember_last_opened(&db, &record.id);
  Ok((record, inspection))
}

/// The generation repositories are keyed by: managed endpoints carry it,
/// user-managed ones are always 0.
pub fn endpoint_generation(endpoint: &SshEndpoint) -> u64 {
  match endpoint.source {
    SshEndpointSource::Managed { generation, .. } => generation,
    _ => 0,
  }
}

pub fn session_snapshot(db: &Database) -> Option<SessionSnapshot> {
  serde_json::from_str(&setting(db, SESSION)?).ok()
}

pub fn save_session_snapshot(db: &Database, snapshot: Option<&SessionSnapshot>) {
  let json = snapshot
    .and_then(|s| serde_json::to_string(s).ok())
    .unwrap_or_default();
  set_setting(db, SESSION, &json);
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
  fn keeps_the_ten_latest_local_repositories_newest_first() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::new(dir.path().join("treq.db")).unwrap();
    db.init().unwrap();
    for i in 0..12 {
      remember_local_opened(&db, &format!("/r/{i}"));
    }
    remember_local_opened(&db, "/r/5");
    let recent = recent_local_paths(&db);
    assert_eq!(recent.len(), 10);
    assert_eq!(recent[..3], ["/r/5", "/r/11", "/r/10"]);
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
      terminal_backend: Default::default(),
    }
    .to_endpoint();
    assert_eq!(endpoint.host_keys[0].fingerprint_sha256, "SHA256:abc");
    assert_eq!(endpoint.source, SshEndpointSource::UserManaged);
  }

  #[test]
  fn a_machine_keeps_its_terminal_backend_and_older_records_use_the_daemon() {
    use crate::core::pty_remote_supervisor::PtyBackend;
    let (_dir, db) = database();
    // A record saved before the setting existed.
    set_setting(
      &db,
      USER_MANAGED_ENDPOINTS,
      r#"[{"id":"u1","display_name":"box","hostname":"box.local","port":22,"username":"me","host_key_fingerprint":"SHA256:abc","auth_identity_reference":"keystore:device","created_at":"2026-01-01T00:00:00Z"}]"#,
    );
    let saved = &user_managed_endpoints(&db)[0];
    assert_eq!(saved.terminal_backend, PtyBackend::Daemon);

    assert!(set_terminal_backend(&db, "u1", PtyBackend::Tmux));
    let saved = &user_managed_endpoints(&db)[0];
    assert_eq!(saved.terminal_backend, PtyBackend::Tmux);
    assert_eq!(saved.to_endpoint().terminal_backend, PtyBackend::Tmux);
    assert!(!set_terminal_backend(&db, "missing", PtyBackend::Tmux));
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

  #[test]
  fn saves_repositories_most_recent_first() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::new(dir.path().join("treq.db")).unwrap();
    db.init().unwrap();
    upsert_saved_repository(&db, "e", 1, "~/a");
    upsert_saved_repository(&db, "e", 1, "~/b/");
    upsert_saved_repository(&db, "e", 1, "~/a");
    upsert_saved_repository(&db, "other", 1, "~/c");
    let paths: Vec<_> = saved_repositories(&db, "e", 1)
      .into_iter()
      .map(|r| r.canonical_remote_path)
      .collect();
    assert_eq!(paths, ["~/a", "~/b"]);
    assert!(saved_repositories(&db, "e", 2).is_empty());
  }

  // -- `remote-repository.test.ts` and `remote-endpoints.ts` ---------------

  fn database() -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::new(dir.path().join("treq.db")).unwrap();
    db.init().unwrap();
    (dir, db)
  }

  fn save(db: &Database, generation: u64, path: &str, trust: Option<&str>) -> SavedRepository {
    upsert(
      db,
      "ep-1",
      generation,
      path,
      None,
      trust.map(|t| Some(t.to_string())),
    )
    .unwrap()
  }

  fn inspection(root: &str) -> RepositoryInspection {
    serde_json::from_value(serde_json::json!({
      "root": root, "repository_type": "jj_colocated", "current_branch": "main",
      "default_branch": "main", "current_change_id": "c", "current_commit_id": "d",
      "descriptor": {"id": "x", "location": {"type": "ssh", "host": "box", "path": root}, "display_name": root}
    }))
    .unwrap()
  }

  #[test]
  fn collapses_slashes_dots_and_trailing_separators() {
    assert_eq!(
      canonicalize_remote_path(" /srv//project/./app/ "),
      "/srv/project/app"
    );
    assert_eq!(canonicalize_remote_path("~/src/../src/app"), "~/src/app");
  }

  #[test]
  fn stores_two_repositories_on_one_endpoint_generation() {
    let (_dir, db) = database();
    save(&db, 3, "/srv/alpha", None);
    save(&db, 3, "/srv/beta", None);
    let mut paths: Vec<_> = saved_repositories(&db, "ep-1", 3)
      .into_iter()
      .map(|r| r.canonical_remote_path)
      .collect();
    paths.sort();
    assert_eq!(paths, ["/srv/alpha", "/srv/beta"]);
  }

  #[test]
  fn deduplicates_the_same_canonical_path_on_one_endpoint_generation() {
    let (_dir, db) = database();
    let first = save(&db, 1, "/srv/project/", None);
    let second = save(&db, 1, "/srv/project", None);
    assert_eq!(second.id, first.id);
    assert_eq!(saved_repositories(&db, "ep-1", 1).len(), 1);
  }

  #[test]
  fn isolates_the_same_path_across_endpoint_generations() {
    let (_dir, db) = database();
    save(&db, 1, "/srv/project", None);
    save(&db, 2, "/srv/project", None);
    assert_eq!(saved_repositories(&db, "ep-1", 1).len(), 1);
    assert_eq!(saved_repositories(&db, "ep-1", 2).len(), 1);
  }

  #[test]
  fn never_persists_credentials_or_private_keys_on_descriptors() {
    let (_dir, db) = database();
    let record = upsert(&db, "ep-1", 1, "/srv/project", Some("project"), None).unwrap();
    assert!(is_secret_free(&record));
    let persisted = db.get_setting(SAVED_REPOSITORIES).unwrap().unwrap();
    for word in [
      "private_key",
      "password",
      "passphrase",
      "credential",
      "begin openssh",
    ] {
      assert!(!persisted.to_lowercase().contains(word), "{word}");
    }
    let first = &serde_json::from_str::<serde_json::Value>(&persisted).unwrap()[0];
    assert_eq!(first["endpoint_id"], "ep-1");
    assert_eq!(first["endpoint_generation"], 1);
    assert_eq!(first["canonical_remote_path"], "/srv/project");
    assert!(first.get("auth_identity_reference").is_none());
    assert!(upsert(&db, "ep-1", 1, "/srv/x", Some("my password"), None).is_err());
    assert!(upsert(&db, "ep-1", 1, "  ", None, None).is_err());
  }

  #[test]
  fn keeps_remote_recents_on_a_distinct_settings_key_from_local_last_opened() {
    let (_dir, db) = database();
    let record = save(&db, 1, "/srv/project", None);
    remember_last_opened(&db, &record.id);
    assert_eq!(db.get_setting("last_opened_repo_path").unwrap(), None);
    assert_eq!(last_opened_id(&db), Some(record.id.clone()));
    assert_eq!(recent_repository_ids(&db), [record.id]);
  }

  /// Records which restore steps ran.
  struct Steps {
    reconnect: bool,
    host_key: bool,
    order: std::cell::RefCell<Vec<&'static str>>,
  }

  impl Steps {
    fn new(reconnect: bool, host_key: bool) -> Self {
      Self {
        reconnect,
        host_key,
        order: Default::default(),
      }
    }
  }

  impl RestoreSteps for Steps {
    fn reconnect(&self) -> bool {
      self.order.borrow_mut().push("reconnect");
      self.reconnect
    }
    fn validate_host_key(&self) -> bool {
      self.order.borrow_mut().push("host_key");
      self.host_key
    }
    fn inspect(&self, canonical_path: &str) -> Result<RepositoryInspection, String> {
      self.order.borrow_mut().push("inspect");
      Ok(inspection(canonical_path))
    }
  }

  const NOW: &str = "2026-10-20T00:00:00.000Z";

  #[test]
  fn restores_only_after_reconnect_host_key_generation_and_inspect_succeed() {
    let (_dir, db) = database();
    let descriptor = save(&db, 4, "/srv/project", None);
    let steps = Steps::new(true, true);
    let (record, inspection) =
      restore_saved_repository(|| &db, &descriptor, 4, false, &steps, NOW).unwrap();
    assert_eq!(*steps.order.borrow(), ["reconnect", "host_key", "inspect"]);
    assert_eq!(
      record.last_successful_trust_validation.as_deref(),
      Some(NOW)
    );
    assert_eq!(inspection.root, "/srv/project");
    assert_eq!(last_opened_id(&db), Some(record.id));
  }

  #[test]
  fn keeps_the_repository_closed_after_a_failed_reconnect() {
    let (_dir, db) = database();
    let descriptor = save(&db, 1, "/srv/project", None);
    let steps = Steps::new(false, true);
    assert_eq!(
      restore_saved_repository(|| &db, &descriptor, 1, false, &steps, NOW).unwrap_err(),
      RestoreFailure::ReconnectFailed
    );
    assert!(!steps.order.borrow().contains(&"inspect"));
  }

  #[test]
  fn refuses_restore_on_host_key_mismatch_without_inspecting() {
    let (_dir, db) = database();
    let descriptor = save(&db, 1, "/srv/project", None);
    let steps = Steps::new(true, false);
    assert_eq!(
      restore_saved_repository(|| &db, &descriptor, 1, false, &steps, NOW).unwrap_err(),
      RestoreFailure::HostKeyMismatch
    );
    assert!(!steps.order.borrow().contains(&"inspect"));
  }

  #[test]
  fn refuses_restore_and_invalidates_trust_after_a_generation_change() {
    let (_dir, db) = database();
    let descriptor = save(&db, 1, "/srv/project", Some("2026-01-01T00:00:00.000Z"));
    let steps = Steps::new(true, true);
    assert_eq!(
      restore_saved_repository(|| &db, &descriptor, 2, false, &steps, NOW).unwrap_err(),
      RestoreFailure::GenerationMismatch
    );
    assert!(!steps.order.borrow().contains(&"inspect"));
    let stale = &saved_repositories(&db, "ep-1", 1)[0];
    assert_eq!(stale.last_successful_trust_validation, None);
  }

  #[test]
  fn accepts_an_explicit_generation_transition_then_records_the_new_generation() {
    let (_dir, db) = database();
    let descriptor = save(&db, 1, "/srv/project", None);
    let steps = Steps::new(true, true);
    let (record, _) = restore_saved_repository(|| &db, &descriptor, 2, true, &steps, NOW).unwrap();
    assert_eq!(record.endpoint_generation, 2);
  }

  #[test]
  fn clears_last_successful_validation_for_the_previous_generation_only() {
    let (_dir, db) = database();
    save(&db, 1, "/srv/a", Some("2026-01-01T00:00:00.000Z"));
    save(&db, 2, "/srv/a", Some("2026-02-01T00:00:00.000Z"));
    invalidate_trust_after_generation_change(&db, "ep-1", 1);
    assert_eq!(
      saved_repositories(&db, "ep-1", 1)[0].last_successful_trust_validation,
      None
    );
    assert_eq!(
      saved_repositories(&db, "ep-1", 2)[0]
        .last_successful_trust_validation
        .as_deref(),
      Some("2026-02-01T00:00:00.000Z")
    );
  }

  #[test]
  fn clearing_the_last_opened_remote_repository_keeps_the_local_path() {
    let (_dir, db) = database();
    db.set_setting("last_opened_repo_path", "/local/repo")
      .unwrap();
    remember_last_opened(&db, "remote-1");
    clear_last_opened(&db);
    assert_eq!(
      db.get_setting("last_opened_repo_path").unwrap().as_deref(),
      Some("/local/repo")
    );
    assert_eq!(last_opened_id(&db), None);
  }

  #[test]
  fn saves_user_managed_endpoints_most_recent_first() {
    let (_dir, db) = database();
    let endpoint = |id: &str| UserManagedEndpoint {
      id: id.into(),
      display_name: id.into(),
      hostname: "box".into(),
      port: 22,
      username: "me".into(),
      host_key_fingerprint: "SHA256:abc".into(),
      auth_identity_reference: "/k/id_ed25519".into(),
      alias: None,
      created_at: NOW.into(),
      terminal_backend: Default::default(),
    };
    save_user_managed_endpoint(&db, endpoint("a"));
    save_user_managed_endpoint(&db, endpoint("b"));
    save_user_managed_endpoint(&db, endpoint("a"));
    let ids: Vec<_> = user_managed_endpoints(&db)
      .into_iter()
      .map(|e| e.id)
      .collect();
    assert_eq!(ids, ["a", "b"]);
  }
}
