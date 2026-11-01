//! Config sync: the user's app settings and each repository's `.treq`
//! config, kept server side (`put_user_config` / `get_user_config`) so a
//! new install or a new cloud machine starts with them.
//!
//! What is uploaded is an allowlist, never "everything but": app settings
//! exported as JSON from the app DB (never the DB file), `skills/`, and per
//! repository `.treq/config.yaml`, `.treq/workflows/`, `.treq/skills/`,
//! `.treq/review-prompt`, `.treq/send`, `.treq/resolve` and
//! `.treq/linear-review`. Credentials, tokens, `*.db`, agent logs and chats,
//! PTY state and workspaces are refused even inside an allowed directory.
//!
//! A snapshot is `{relative path: base64}`, at most 1 MiB in all. Writes
//! are compare-and-swap on the scope's version; on a conflict the client
//! re-reads, merges (last writer wins per file) and retries once. The
//! network sits behind [`ConfigTransport`], so the flow runs against a fake
//! in tests.

use std::collections::BTreeMap;
use std::path::{Component, Path};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use base64::Engine as _;
use serde_json::{json, Value};

use super::feature_preview::PreviewFeature;
use super::supabase::Supabase;

/// The app-wide scope.
pub const APP_SCOPE: &str = "app";
/// The server's limit on one snapshot.
pub const MAX_SNAPSHOT_BYTES: usize = 1024 * 1024;
/// Where the exported app settings sit in the app snapshot.
pub const SETTINGS_FILE: &str = "settings.json";
/// A change is pushed once the files have been still this long.
pub const DEBOUNCE: Duration = Duration::from_secs(10);
/// Files are pushed at least this often, changed or not.
pub const PERIODIC: Duration = Duration::from_secs(60 * 60);
/// How often the files are checked for a change.
pub const POLL: Duration = Duration::from_secs(5);

/// Relative path to file bytes.
pub type Files = BTreeMap<String, Vec<u8>>;

/// The app settings that follow the user. Secrets (the Linear API key, the
/// Supabase session), machine paths and run state stay local.
const APP_SETTING_KEYS: &[&str] = &[
  "theme",
  "ui_zoom",
  "terminal_font_size",
  "default_model",
  "default_agent",
  "conflict_marker_style",
  "notify_agent_finished",
  "installed_skills",
  super::app_setup_script::SCRIPT_KEY,
  super::app_setup_script::ALWAYS_RUN_KEY,
];

const PREVIEW_FEATURES: [PreviewFeature; 6] = [
  PreviewFeature::SkillsInstallation,
  PreviewFeature::RemoteSsh,
  PreviewFeature::LinearIntegration,
  PreviewFeature::Logs,
  PreviewFeature::Devcontainers,
  PreviewFeature::Byos,
];

/// Every settings key that is synced.
pub fn synced_setting_keys() -> Vec<String> {
  APP_SETTING_KEYS
    .iter()
    .map(|k| k.to_string())
    .chain(PREVIEW_FEATURES.iter().map(|f| f.setting_key()))
    .collect()
}

/// The app directory's synced directories.
const APP_DIRS: &[&str] = &["skills"];

/// A repository's synced files and directories.
const REPO_ENTRIES: &[&str] = &[
  ".treq/config.yaml",
  ".treq/workflows",
  ".treq/skills",
  ".treq/review-prompt",
  ".treq/send",
  ".treq/resolve",
  ".treq/linear-review",
];

/// Path parts never synced, wherever they appear.
const DENIED_DIRS: &[&str] = &["agents", "agent-chats", "pty", "workspaces", ".git", ".jj"];

/// A relative path that is plain (no `..`, not absolute, `/`-separated)
/// and names nothing secret or local-only.
fn is_safe(rel: &str) -> bool {
  if rel.is_empty() || rel.contains('\\') {
    return false;
  }
  let path = Path::new(rel);
  if !path.components().all(|c| matches!(c, Component::Normal(_))) {
    return false;
  }
  let parts: Vec<&str> = rel.split('/').collect();
  if parts
    .iter()
    .any(|p| p.is_empty() || DENIED_DIRS.contains(p))
  {
    return false;
  }
  let name = parts.last().unwrap_or(&"").to_ascii_lowercase();
  let secret = ["token", "secret", "credential", "password", "session"]
    .iter()
    .any(|w| name.contains(w));
  let db = name.ends_with(".db") || name.contains(".db-") || name.ends_with(".sqlite");
  let key = name.starts_with(".env")
    || name.ends_with(".pem")
    || name.ends_with(".key")
    || name.starts_with("id_rsa")
    || name.starts_with("id_ed25519");
  !(secret || db || key)
}

fn within(rel: &str, entry: &str) -> bool {
  rel == entry || rel.starts_with(&format!("{entry}/"))
}

/// Whether `rel` (relative to the app directory) may be synced.
pub fn is_allowed_app_path(rel: &str) -> bool {
  is_safe(rel)
    && (rel == SETTINGS_FILE || APP_DIRS.iter().any(|d| rel.starts_with(&format!("{d}/"))))
}

/// Whether `rel` (relative to the repository root) may be synced.
pub fn is_allowed_repo_path(rel: &str) -> bool {
  is_safe(rel) && REPO_ENTRIES.iter().any(|e| within(rel, e))
}

/// Adds the files under `root/rel` (a file or a directory) that `allowed`
/// accepts. Symlinks are skipped, so nothing outside `root` is read.
fn collect(root: &Path, rel: &str, allowed: fn(&str) -> bool, files: &mut Files) {
  let path = root.join(rel);
  let Ok(meta) = std::fs::symlink_metadata(&path) else {
    return;
  };
  if meta.is_file() {
    if allowed(rel) {
      if let Ok(bytes) = std::fs::read(&path) {
        files.insert(rel.to_string(), bytes);
      }
    }
  } else if meta.is_dir() {
    let Ok(entries) = std::fs::read_dir(&path) else {
      return;
    };
    for entry in entries.flatten() {
      if let Some(name) = entry.file_name().to_str() {
        collect(root, &format!("{rel}/{name}"), allowed, files);
      }
    }
  }
}

/// The app snapshot: the synced settings as JSON, and `skills/`.
pub fn collect_app(app_dir: &Path, settings: &BTreeMap<String, String>) -> Files {
  let keys = synced_setting_keys();
  let synced: BTreeMap<&String, &String> =
    settings.iter().filter(|(k, _)| keys.contains(k)).collect();
  let mut files = Files::new();
  if !synced.is_empty() {
    files.insert(
      SETTINGS_FILE.to_string(),
      serde_json::to_vec_pretty(&synced).unwrap_or_default(),
    );
  }
  for dir in APP_DIRS {
    collect(app_dir, dir, is_allowed_app_path, &mut files);
  }
  files
}

/// A repository's snapshot: its synced `.treq` config.
pub fn collect_repo(repo: &Path) -> Files {
  let mut files = Files::new();
  for entry in REPO_ENTRIES {
    collect(repo, entry, is_allowed_repo_path, &mut files);
  }
  files
}

/// The synced settings in an app snapshot.
pub fn settings_of(files: &Files) -> BTreeMap<String, String> {
  let keys = synced_setting_keys();
  files
    .get(SETTINGS_FILE)
    .and_then(|b| serde_json::from_slice::<BTreeMap<String, String>>(b).ok())
    .unwrap_or_default()
    .into_iter()
    .filter(|(k, _)| keys.contains(k))
    .collect()
}

/// Writes the files `allowed` accepts under `root`. `settings.json` is not
/// a file on disk; read it with [`settings_of`].
///
/// A pulled snapshot can come from a repository's committed `.treq`, so no
/// symlink below `root` is followed: every directory on the way and the
/// file itself are checked with `symlink_metadata`, and the bytes go to a
/// temp file in the same directory that is renamed into place.
pub fn write_files(root: &Path, files: &Files, allowed: fn(&str) -> bool) -> Result<(), String> {
  for (rel, bytes) in files {
    if rel == SETTINGS_FILE || !allowed(rel) {
      continue;
    }
    write_one(root, rel, bytes).map_err(|e| format!("Failed to write {rel}: {e}"))?;
  }
  Ok(())
}

fn write_one(root: &Path, rel: &str, bytes: &[u8]) -> Result<(), String> {
  let parts: Vec<&str> = rel.split('/').collect();
  let (name, dirs) = parts.split_last().ok_or("empty path")?;
  let mut dir = root.to_path_buf();
  for part in dirs {
    dir.push(part);
    match std::fs::symlink_metadata(&dir) {
      Ok(meta) if meta.file_type().is_symlink() => return Err("a parent is a symlink".into()),
      Ok(meta) if !meta.is_dir() => return Err("a parent is not a directory".into()),
      Ok(_) => {}
      Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
        std::fs::create_dir(&dir).map_err(|e| e.to_string())?
      }
      Err(e) => return Err(e.to_string()),
    }
  }
  let path = dir.join(name);
  if let Ok(meta) = std::fs::symlink_metadata(&path) {
    if meta.file_type().is_symlink() {
      return Err("the file is a symlink".into());
    }
  }
  let tmp = dir.join(format!(".{name}.treq-sync-{}", std::process::id()));
  let written = (|| {
    use std::io::Write as _;
    let mut file = std::fs::OpenOptions::new()
      .write(true)
      .create_new(true)
      .open(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    std::fs::rename(&tmp, &path)
  })();
  if written.is_err() {
    std::fs::remove_file(&tmp).ok();
  }
  written.map_err(|e| e.to_string())
}

/// The scope of a repository: `repo:` and its remote's URL. `None` for a
/// repository without a GitHub remote, which is not synced.
pub fn repo_scope(repo_path: &str) -> Option<String> {
  let remote = crate::github::get_git_remote_url_impl(repo_path).ok()??;
  Some(format!("repo:https://github.com/{}", remote.full_name))
}

/// `{path: base64}`, refusing a snapshot over the server's limit.
pub fn encode(files: &Files) -> Result<Value, String> {
  let total: usize = files.values().map(Vec::len).sum();
  if total > MAX_SNAPSHOT_BYTES {
    return Err(format!(
      "config snapshot is {total} bytes, over the {MAX_SNAPSHOT_BYTES}-byte limit"
    ));
  }
  let engine = base64::engine::general_purpose::STANDARD;
  Ok(Value::Object(
    files
      .iter()
      .map(|(k, v)| (k.clone(), Value::String(engine.encode(v))))
      .collect(),
  ))
}

/// The files of a snapshot, skipping entries that are not valid base64.
pub fn decode(value: &Value) -> Files {
  let engine = base64::engine::general_purpose::STANDARD;
  value
    .as_object()
    .map(|map| {
      map
        .iter()
        .filter_map(|(k, v)| Some((k.clone(), engine.decode(v.as_str()?).ok()?)))
        .collect()
    })
    .unwrap_or_default()
}

/// Path to SHA-256 (hex) of each file as last synced: the base of the
/// three-way merge, kept locally per scope.
pub type Manifest = BTreeMap<String, String>;

fn sha256_hex(bytes: &[u8]) -> String {
  use sha2::{Digest, Sha256};
  Sha256::digest(bytes)
    .iter()
    .map(|b| format!("{b:02x}"))
    .collect()
}

pub fn manifest(files: &Files) -> Manifest {
  files
    .iter()
    .map(|(k, v)| (k.clone(), sha256_hex(v)))
    .collect()
}

/// Last writer wins per file, local first, against `base`, the last synced
/// manifest, so deletions propagate: a file only `remote` has that was in
/// `base` was deleted here and is dropped; a file only `local` has that is
/// unchanged since `base` was deleted on the server and is dropped too.
/// With an empty base, every file of either side is kept.
pub fn merge3(base: &Manifest, remote: &Files, local: &Files) -> Files {
  let local_hashes = manifest(local);
  let mut merged = Files::new();
  for (k, v) in remote {
    if !local.contains_key(k) && !base.contains_key(k) {
      merged.insert(k.clone(), v.clone());
    }
  }
  for (k, v) in local {
    let deleted_remotely = !remote.contains_key(k) && base.get(k) == local_hashes.get(k);
    if !deleted_remotely {
      merged.insert(k.clone(), v.clone());
    }
  }
  merged
}

fn manifest_file(dir: &Path, scope: &str) -> std::path::PathBuf {
  let mut name = sha256_hex(scope.as_bytes());
  name.truncate(32);
  dir.join(format!("{name}.json"))
}

/// The last synced manifest of `scope` in `dir`; empty when none is stored.
pub fn load_manifest(dir: &Path, scope: &str) -> Manifest {
  std::fs::read(manifest_file(dir, scope))
    .ok()
    .and_then(|b| serde_json::from_slice(&b).ok())
    .unwrap_or_default()
}

pub fn save_manifest(dir: &Path, scope: &str, manifest: &Manifest) -> Result<(), String> {
  std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
  let body = serde_json::to_vec(manifest).map_err(|e| e.to_string())?;
  std::fs::write(manifest_file(dir, scope), body).map_err(|e| e.to_string())
}

/// The newest stored version of a scope.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
  pub version: u64,
  pub files: Files,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SyncError {
  /// Another writer stored a version since it was read.
  Conflict,
  Other(String),
}

impl std::fmt::Display for SyncError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::Conflict => write!(f, "version_conflict"),
      Self::Other(error) => write!(f, "{error}"),
    }
  }
}

/// Where snapshots are stored.
#[async_trait]
pub trait ConfigTransport: Send + Sync {
  async fn get(&self, scope: &str) -> Result<Option<Snapshot>, SyncError>;
  /// Stores `files` as the version after `expected_version`; returns the
  /// new version.
  async fn put(&self, scope: &str, expected_version: u64, files: &Files) -> Result<u64, SyncError>;
}

/// The Supabase RPCs.
pub struct SupabaseTransport(pub Arc<Supabase>);

#[async_trait]
impl ConfigTransport for SupabaseTransport {
  async fn get(&self, scope: &str) -> Result<Option<Snapshot>, SyncError> {
    let value: Value = self
      .0
      .rpc("get_user_config", &json!({ "p_scope": scope }))
      .await
      .map_err(|e| SyncError::Other(e.to_string()))?;
    if value.is_null() {
      return Ok(None);
    }
    Ok(Some(Snapshot {
      version: value["version"].as_u64().unwrap_or(0),
      files: decode(&value["files"]),
    }))
  }

  async fn put(&self, scope: &str, expected_version: u64, files: &Files) -> Result<u64, SyncError> {
    let files = encode(files).map_err(SyncError::Other)?;
    let args = json!({
      "p_scope": scope, "p_expected_version": expected_version, "p_files": files
    });
    self
      .0
      .rpc::<u64>("put_user_config", &args)
      .await
      .map_err(|e| {
        if e.message.contains("version_conflict") || e.code.contains("version_conflict") {
          SyncError::Conflict
        } else {
          SyncError::Other(e.to_string())
        }
      })
  }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PushOutcome {
  /// The server already had these files.
  Unchanged(u64),
  Pushed(u64),
}

/// Pushes `local`, three-way merged ([`merge3`]) over what the server has.
/// On a version conflict re-reads, merges again and retries once. Returns
/// the manifest of what the server now holds, the next base.
pub async fn push(
  transport: &dyn ConfigTransport,
  scope: &str,
  local: &Files,
  base: &Manifest,
) -> Result<(PushOutcome, Manifest), SyncError> {
  let mut retry = false;
  loop {
    let remote = transport.get(scope).await?;
    let (version, remote_files) = remote.map_or((0, Files::new()), |s| (s.version, s.files));
    let merged = merge3(base, &remote_files, local);
    if !retry && version > 0 && merged == remote_files {
      return Ok((PushOutcome::Unchanged(version), manifest(&merged)));
    }
    match transport.put(scope, version, &merged).await {
      Err(SyncError::Conflict) if !retry => retry = true,
      result => return result.map(|v| (PushOutcome::Pushed(v), manifest(&merged))),
    }
  }
}

/// The server's files for `scope` when the local copy is missing, else
/// `None` (a local copy is never overwritten by a pull).
pub async fn pull_if_missing(
  transport: &dyn ConfigTransport,
  scope: &str,
  local_missing: bool,
) -> Result<Option<Files>, SyncError> {
  if !local_missing {
    return Ok(None);
  }
  Ok(transport.get(scope).await?.map(|s| s.files))
}

/// When a scope is due a push: after a change has been still for
/// [`DEBOUNCE`], and every [`PERIODIC`] regardless.
#[derive(Debug, Default)]
pub struct Schedule {
  pushed: Option<(u64, Instant)>,
  changed: Option<(u64, Instant)>,
}

impl Schedule {
  /// Records the files' fingerprint at `now`; true when a push is due.
  pub fn observe(&mut self, fingerprint: u64, now: Instant) -> bool {
    let Some((pushed, at)) = self.pushed else {
      return true;
    };
    if now.duration_since(at) >= PERIODIC {
      return true;
    }
    if fingerprint == pushed {
      self.changed = None;
      return false;
    }
    match self.changed {
      Some((seen, since)) if seen == fingerprint => now.duration_since(since) >= DEBOUNCE,
      _ => {
        self.changed = Some((fingerprint, now));
        false
      }
    }
  }

  pub fn pushed(&mut self, fingerprint: u64, now: Instant) {
    self.pushed = Some((fingerprint, now));
    self.changed = None;
  }
}

pub fn fingerprint(files: &Files) -> u64 {
  use std::hash::{Hash, Hasher};
  let mut hasher = std::collections::hash_map::DefaultHasher::new();
  files.hash(&mut hasher);
  hasher.finish()
}

/// Writes pulled files into place.
pub type ApplyFn = Box<dyn Fn(&Files) -> Result<(), String> + Send + Sync>;

/// One synced scope: how to read its files, whether the local copy is
/// missing, and how to apply pulled files.
pub struct Source {
  pub scope: String,
  pub collect: Box<dyn Fn() -> Files + Send + Sync>,
  pub missing: Box<dyn Fn() -> bool + Send + Sync>,
  pub apply: ApplyFn,
}

/// Pulls each scope whose local copy is missing, then pushes on change
/// (debounced) and hourly. Each scope's last synced manifest is kept in
/// `state_dir` so local deletions reach the server. Runs until aborted.
pub async fn run(
  transport: Arc<dyn ConfigTransport>,
  sources: Vec<Source>,
  state_dir: std::path::PathBuf,
) {
  for source in &sources {
    match pull_if_missing(transport.as_ref(), &source.scope, (source.missing)()).await {
      Ok(Some(files)) => {
        if let Err(error) = save_manifest(&state_dir, &source.scope, &manifest(&files)) {
          log::warn!(
            "saving the synced manifest for {} failed: {error}",
            source.scope
          );
        }
        if let Err(error) = (source.apply)(&files) {
          log::warn!(
            "applying synced config for {} failed: {error}",
            source.scope
          );
        }
      }
      Ok(None) => {}
      Err(error) => log::warn!("pulling synced config for {} failed: {error}", source.scope),
    }
  }
  let mut schedules: Vec<Schedule> = sources.iter().map(|_| Schedule::default()).collect();
  loop {
    for (source, schedule) in sources.iter().zip(&mut schedules) {
      let files = (source.collect)();
      let print = fingerprint(&files);
      if !schedule.observe(print, Instant::now()) {
        continue;
      }
      let base = load_manifest(&state_dir, &source.scope);
      match push(transport.as_ref(), &source.scope, &files, &base).await {
        Ok((_, synced)) => {
          if let Err(error) = save_manifest(&state_dir, &source.scope, &synced) {
            log::warn!(
              "saving the synced manifest for {} failed: {error}",
              source.scope
            );
          }
          schedule.pushed(print, Instant::now());
        }
        Err(error) => {
          log::warn!("pushing synced config for {} failed: {error}", source.scope);
          // Retry at the next periodic push, not every poll.
          schedule.pushed(print, Instant::now());
        }
      }
    }
    tokio::time::sleep(POLL).await;
  }
}

/// The app scope's source: settings from the app DB, `skills/` from the
/// app directory.
pub fn app_source(app_dir: std::path::PathBuf) -> Source {
  let db_path = app_dir.join("treq.db");
  let read_settings = {
    let db_path = db_path.clone();
    move || -> BTreeMap<String, String> {
      let Ok(db) = crate::db::Database::new(db_path.clone()) else {
        return BTreeMap::new();
      };
      db.get_settings_batch(&synced_setting_keys())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(k, v)| Some((k, v?)))
        .collect()
    }
  };
  let collect_dir = app_dir.clone();
  let missing_settings = read_settings.clone();
  let missing_dir = app_dir.clone();
  Source {
    scope: APP_SCOPE.to_string(),
    collect: Box::new(move || collect_app(&collect_dir, &read_settings())),
    missing: Box::new(move || {
      missing_settings().is_empty() && collect_app(&missing_dir, &BTreeMap::new()).is_empty()
    }),
    apply: Box::new(move |files| {
      write_files(&app_dir, files, is_allowed_app_path)?;
      let db = crate::db::Database::new(db_path.clone()).map_err(|e| e.to_string())?;
      for (key, value) in settings_of(files) {
        db.set_setting(&key, &value).map_err(|e| e.to_string())?;
      }
      Ok(())
    }),
  }
}

/// A repository's source, or `None` when it has no remote to scope it by.
pub fn repo_source(repo_path: &str) -> Option<Source> {
  let scope = repo_scope(repo_path)?;
  let root = std::path::PathBuf::from(repo_path);
  let (collect_root, missing_root) = (root.clone(), root.clone());
  Some(Source {
    scope,
    collect: Box::new(move || collect_repo(&collect_root)),
    missing: Box::new(move || !missing_root.join(".treq/config.yaml").exists()),
    apply: Box::new(move |files| write_files(&root, files, is_allowed_repo_path)),
  })
}

#[cfg(test)]
mod tests {
  use std::sync::Mutex;

  use super::*;

  fn write(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
  }

  #[test]
  fn collects_only_the_allowlisted_repo_config() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for rel in [
      ".treq/config.yaml",
      ".treq/workflows/ci.yaml",
      ".treq/skills/review/SKILL.md",
      ".treq/review-prompt",
      ".treq/send/default.md",
      ".treq/resolve",
      ".treq/linear-review",
      ".treq/local.db",
      ".treq/agents/a1/log.jsonl",
      ".treq/agent-chats/c.json",
      ".treq/pty/1.sock",
      ".treq/workspaces/ws-1/file.rs",
      ".treq/skills/review/.env",
      ".treq/workflows/github_token",
      ".treq/other.yaml",
      "src/main.rs",
    ] {
      write(root, rel, "x");
    }
    let files = collect_repo(root);
    let paths: Vec<&str> = files.keys().map(String::as_str).collect();
    assert_eq!(
      paths,
      [
        ".treq/config.yaml",
        ".treq/linear-review",
        ".treq/resolve",
        ".treq/review-prompt",
        ".treq/send/default.md",
        ".treq/skills/review/SKILL.md",
        ".treq/workflows/ci.yaml",
      ]
    );
  }

  #[test]
  fn never_allows_denied_paths() {
    for rel in [
      ".treq/local.db",
      ".treq/skills/x.db-wal",
      ".treq/skills/agents/x.md",
      ".treq/workflows/agent-chats/x",
      ".treq/skills/pty/x",
      ".treq/skills/workspaces/x",
      ".treq/skills/../agents/x",
      "/etc/passwd",
      ".treq/workflows/credentials.json",
      ".treq/skills/id_ed25519",
      ".treq/skills/server.pem",
      ".treq/agents",
      ".treq/pty",
    ] {
      assert!(!is_allowed_repo_path(rel), "{rel}");
    }
    for rel in [
      "treq.db",
      "skills/x/session.json",
      "skills/../treq.db",
      "remote/keys/id_ed25519",
      "agents/x",
    ] {
      assert!(!is_allowed_app_path(rel), "{rel}");
    }
  }

  #[test]
  fn exports_only_synced_settings_and_app_skills() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "skills/mine/SKILL.md", "skill");
    write(dir.path(), "treq.db", "db");
    write(dir.path(), "agents/a/log", "log");
    let settings: BTreeMap<String, String> = [
      ("theme", "dark"),
      ("linear_api_key", "lin_secret"),
      ("supabase_session", "{\"accessToken\":\"t\"}"),
      ("last_opened_repo_path", "/home/me/repo"),
      ("app_setup_script", "brew install jq"),
      ("feature_preview.logs", "true"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    let files = collect_app(dir.path(), &settings);
    assert_eq!(
      files.keys().collect::<Vec<_>>(),
      ["settings.json", "skills/mine/SKILL.md"]
    );
    let exported = String::from_utf8(files["settings.json"].clone()).unwrap();
    assert!(!exported.contains("lin_secret"));
    assert!(!exported.contains("accessToken"));
    assert!(!exported.contains("/home/me/repo"));
    let synced = settings_of(&files);
    assert_eq!(synced["theme"], "dark");
    assert_eq!(synced["app_setup_script"], "brew install jq");
    assert_eq!(synced["feature_preview.logs"], "true");
    assert_eq!(synced.len(), 3);
  }

  #[test]
  fn writes_pulled_files_but_skips_denied_ones() {
    let dir = tempfile::tempdir().unwrap();
    let files = files(&[
      (".treq/config.yaml", "a"),
      (".treq/agents/x", "b"),
      (".treq/../../escape", "c"),
    ]);
    write_files(dir.path(), &files, is_allowed_repo_path).unwrap();
    assert!(dir.path().join(".treq/config.yaml").exists());
    assert!(!dir.path().join(".treq/agents/x").exists());
  }

  #[cfg(unix)]
  #[test]
  fn refuses_to_write_through_a_symlinked_file() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("bashrc");
    std::fs::write(&target, "original").unwrap();
    std::fs::create_dir_all(dir.path().join(".treq")).unwrap();
    std::os::unix::fs::symlink(&target, dir.path().join(".treq/send")).unwrap();
    let files = files(&[(".treq/send", "pwned")]);
    assert!(write_files(dir.path(), &files, is_allowed_repo_path).is_err());
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "original");
  }

  #[cfg(unix)]
  #[test]
  fn refuses_to_write_through_a_symlinked_directory() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join(".treq")).unwrap();
    let files = files(&[(".treq/workflows/a.yaml", "pwned")]);
    assert!(write_files(dir.path(), &files, is_allowed_repo_path).is_err());
    assert!(!outside.path().join("workflows/a.yaml").exists());
  }

  #[test]
  fn replaces_an_existing_file_leaving_no_temp_file() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), ".treq/send", "old");
    write_files(
      dir.path(),
      &files(&[(".treq/send", "new")]),
      is_allowed_repo_path,
    )
    .unwrap();
    assert_eq!(
      std::fs::read_to_string(dir.path().join(".treq/send")).unwrap(),
      "new"
    );
    let names: Vec<_> = std::fs::read_dir(dir.path().join(".treq"))
      .unwrap()
      .map(|e| e.unwrap().file_name())
      .collect();
    assert_eq!(names, vec![std::ffi::OsString::from("send")]);
  }

  #[test]
  fn three_way_merge_propagates_deletions_both_ways() {
    // `b` was synced, then deleted here: it leaves the server.
    // `c` was synced, then deleted on the server: this copy does not bring it back.
    let base = manifest(&files(&[("a", "1"), ("b", "1"), ("c", "1")]));
    let remote = files(&[("a", "1"), ("b", "1"), ("d", "theirs")]);
    let local = files(&[("a", "2"), ("c", "1")]);
    assert_eq!(
      merge3(&base, &remote, &local),
      files(&[("a", "2"), ("d", "theirs")])
    );
  }

  #[test]
  fn three_way_merge_keeps_a_local_edit_of_a_remotely_deleted_file() {
    let base = manifest(&files(&[("c", "1")]));
    assert_eq!(
      merge3(&base, &Files::new(), &files(&[("c", "2")])),
      files(&[("c", "2")])
    );
  }

  #[tokio::test]
  async fn a_local_deletion_reaches_the_server() {
    let transport = FakeTransport::default();
    let first = files(&[("a", "1"), ("b", "1")]);
    let (_, base) = push(&transport, "app", &first, &Manifest::new())
      .await
      .unwrap();
    let (outcome, base) = push(&transport, "app", &files(&[("a", "1")]), &base)
      .await
      .unwrap();
    assert_eq!(outcome, PushOutcome::Pushed(2));
    assert_eq!(
      transport.stored.lock().unwrap().clone().unwrap().files,
      files(&[("a", "1")])
    );
    assert_eq!(base, manifest(&files(&[("a", "1")])));
  }

  #[test]
  fn stores_the_last_synced_manifest_per_scope() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(load_manifest(dir.path(), "repo:x"), Manifest::new());
    let m = manifest(&files(&[("a", "1")]));
    save_manifest(dir.path(), "repo:x", &m).unwrap();
    assert_eq!(load_manifest(dir.path(), "repo:x"), m);
    assert_eq!(load_manifest(dir.path(), "app"), Manifest::new());
  }

  #[test]
  fn round_trips_through_base64_and_refuses_oversized_snapshots() {
    let files: Files = [("a".to_string(), b"hello".to_vec())].into();
    let value = encode(&files).unwrap();
    assert_eq!(value, json!({"a": "aGVsbG8="}));
    assert_eq!(decode(&value), files);
    let big: Files = [("a".to_string(), vec![0; MAX_SNAPSHOT_BYTES + 1])].into();
    assert!(encode(&big).is_err());
  }

  #[test]
  fn merges_with_the_local_file_winning() {
    let remote = files(&[("a", "old"), ("b", "r")]);
    let merged = merge3(&Manifest::new(), &remote, &files(&[("a", "new")]));
    assert_eq!(merged, files(&[("a", "new"), ("b", "r")]));
  }

  #[derive(Default)]
  struct FakeTransport {
    stored: Mutex<Option<Snapshot>>,
    /// Puts to refuse with a conflict, storing this snapshot first (as
    /// another writer would).
    conflicts: Mutex<Vec<Snapshot>>,
    puts: Mutex<Vec<(u64, Files)>>,
  }

  #[async_trait]
  impl ConfigTransport for FakeTransport {
    async fn get(&self, _: &str) -> Result<Option<Snapshot>, SyncError> {
      Ok(self.stored.lock().unwrap().clone())
    }
    async fn put(&self, _: &str, expected: u64, files: &Files) -> Result<u64, SyncError> {
      self.puts.lock().unwrap().push((expected, files.clone()));
      if let Some(other) = self.conflicts.lock().unwrap().pop() {
        *self.stored.lock().unwrap() = Some(other);
        return Err(SyncError::Conflict);
      }
      let mut stored = self.stored.lock().unwrap();
      let current = stored.as_ref().map_or(0, |s| s.version);
      if current != expected {
        return Err(SyncError::Conflict);
      }
      *stored = Some(Snapshot {
        version: current + 1,
        files: files.clone(),
      });
      Ok(current + 1)
    }
  }

  fn files(entries: &[(&str, &str)]) -> Files {
    entries
      .iter()
      .map(|(k, v)| (k.to_string(), v.as_bytes().to_vec()))
      .collect()
  }

  /// [`push`] with no synced base.
  async fn push_fresh(transport: &FakeTransport, local: &Files) -> Result<PushOutcome, SyncError> {
    push(transport, "app", local, &Manifest::new())
      .await
      .map(|(outcome, _)| outcome)
  }

  #[tokio::test]
  async fn pushes_the_first_version_then_skips_unchanged_files() {
    let transport = FakeTransport::default();
    let local = files(&[("a", "1")]);
    assert_eq!(
      push_fresh(&transport, &local).await,
      Ok(PushOutcome::Pushed(1))
    );
    assert_eq!(
      push_fresh(&transport, &local).await,
      Ok(PushOutcome::Unchanged(1))
    );
    assert_eq!(transport.puts.lock().unwrap().len(), 1);
  }

  #[tokio::test]
  async fn retries_once_after_a_conflict_merging_the_other_writers_files() {
    let transport = FakeTransport::default();
    transport.conflicts.lock().unwrap().push(Snapshot {
      version: 1,
      files: files(&[("a", "theirs"), ("b", "theirs")]),
    });
    let outcome = push_fresh(&transport, &files(&[("a", "mine")])).await;
    assert_eq!(outcome, Ok(PushOutcome::Pushed(2)));
    let stored = transport.stored.lock().unwrap().clone().unwrap();
    assert_eq!(stored.files, files(&[("a", "mine"), ("b", "theirs")]));
    let puts = transport.puts.lock().unwrap();
    assert_eq!(puts.len(), 2);
    assert_eq!(puts[1].0, 1);
  }

  #[tokio::test]
  async fn gives_up_after_a_second_conflict() {
    let transport = FakeTransport::default();
    for version in [2, 1] {
      transport.conflicts.lock().unwrap().push(Snapshot {
        version,
        files: Files::new(),
      });
    }
    let outcome = push_fresh(&transport, &files(&[("a", "mine")])).await;
    assert_eq!(outcome, Err(SyncError::Conflict));
    assert_eq!(transport.puts.lock().unwrap().len(), 2);
  }

  #[tokio::test]
  async fn pulls_only_when_the_local_copy_is_missing() {
    let transport = FakeTransport::default();
    *transport.stored.lock().unwrap() = Some(Snapshot {
      version: 3,
      files: files(&[("a", "1")]),
    });
    assert_eq!(pull_if_missing(&transport, "app", false).await, Ok(None));
    assert_eq!(
      pull_if_missing(&transport, "app", true).await,
      Ok(Some(files(&[("a", "1")])))
    );
  }

  #[test]
  fn schedules_a_push_after_the_debounce_and_hourly() {
    let start = Instant::now();
    let mut schedule = Schedule::default();
    assert!(schedule.observe(1, start));
    schedule.pushed(1, start);
    assert!(!schedule.observe(1, start + Duration::from_secs(1)));
    assert!(!schedule.observe(2, start + Duration::from_secs(2)));
    assert!(!schedule.observe(2, start + Duration::from_secs(5)));
    // A further change restarts the debounce.
    assert!(!schedule.observe(3, start + Duration::from_secs(11)));
    assert!(schedule.observe(3, start + Duration::from_secs(21)));
    schedule.pushed(3, start + Duration::from_secs(21));
    assert!(!schedule.observe(3, start + Duration::from_secs(60)));
    assert!(schedule.observe(3, start + Duration::from_secs(21) + PERIODIC));
  }

  #[test]
  fn scopes_a_repository_by_its_remote_url() {
    let dir = tempfile::tempdir().unwrap();
    write(
      dir.path(),
      ".git/config",
      "[remote \"origin\"]\n\turl = git@github.com:acme/api.git\n",
    );
    assert_eq!(
      repo_scope(dir.path().to_str().unwrap()).as_deref(),
      Some("repo:https://github.com/acme/api")
    );
    let bare = tempfile::tempdir().unwrap();
    assert_eq!(repo_scope(bare.path().to_str().unwrap()), None);
  }

  #[tokio::test]
  async fn talks_to_the_config_rpcs() {
    use wiremock::{
      matchers::{method, path},
      Mock, MockServer, ResponseTemplate,
    };
    let server = MockServer::start().await;
    Mock::given(method("POST"))
      .and(path("/rest/v1/rpc/get_user_config"))
      .respond_with(ResponseTemplate::new(200).set_body_json(json!({
        "version": 4, "files": {"a": "aGVsbG8="}, "created_at": "2026-10-01T00:00:00Z"
      })))
      .mount(&server)
      .await;
    Mock::given(method("POST"))
      .and(path("/rest/v1/rpc/put_user_config"))
      .respond_with(ResponseTemplate::new(400).set_body_json(json!({
        "code": "P0001", "message": "version_conflict"
      })))
      .mount(&server)
      .await;
    let (supabase, _) = crate::core::supabase::test_support::client(&server.uri());
    let transport = SupabaseTransport(Arc::new(supabase));
    let snapshot = transport.get("app").await.unwrap().unwrap();
    assert_eq!(snapshot.version, 4);
    assert_eq!(snapshot.files["a"], b"hello");
    assert_eq!(
      transport.put("app", 3, &Files::new()).await,
      Err(SyncError::Conflict)
    );
  }
}
