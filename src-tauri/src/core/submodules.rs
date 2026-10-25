use crate::jj;
use crate::result_ext::ResultExt;
use gix::bstr::{BStr, ByteSlice};
use gix::refs::transaction::{Change, LogChange, PreviousValue, RefEdit};
use gix::refs::Target;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::atomic::AtomicBool;

/// Readonly checkout state of a Git submodule in a superproject working copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubmoduleState {
  Missing,
  Clean,
  Dirty,
  Diverged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitSubmodule {
  pub name: String,
  pub path: String,
  pub url: String,
  pub pin: String,
  pub head: Option<String>,
  pub state: SubmoduleState,
}

/// List Git submodules recorded in the current superproject commit.
///
/// Missing workspace paths return an empty list rather than an IO error.
pub fn list_submodules(
  repo_path: &str,
  workspace_id: Option<i64>,
) -> Result<Vec<GitSubmodule>, String> {
  let workspace_path = resolve_workspace_dir(repo_path, workspace_id)?;
  let root = Path::new(&workspace_path);
  if !root.exists() {
    return Ok(Vec::new());
  }

  let gitmodules = root.join(".gitmodules");
  if !gitmodules.is_file() {
    return Ok(Vec::new());
  }
  let Ok(contents) = fs::read_to_string(&gitmodules) else {
    return Ok(Vec::new());
  };
  let declared = parse_gitmodules(&contents);
  if declared.is_empty() {
    return Ok(Vec::new());
  }

  let git_repo_path =
    jj::derive_repo_path_from_workspace(&workspace_path).unwrap_or_else(|| workspace_path.clone());
  let pins = gitlinks_for_workspace(&workspace_path, &git_repo_path);

  let mut result = Vec::new();
  for spec in declared {
    let pin = pins.get(&spec.path).cloned().unwrap_or_default();
    result.push(inspect_submodule(root, spec, pin));
  }
  Ok(result)
}

/// Clone or fetch each submodule and check out the superproject pin detached.
pub fn update_submodules(
  repo_path: &str,
  workspace_id: Option<i64>,
  path: Option<&str>,
) -> Result<Vec<GitSubmodule>, String> {
  let workspace_path = resolve_workspace_dir(repo_path, workspace_id)?;
  let listed = list_submodules(repo_path, workspace_id)?;
  let git_repo_path =
    jj::derive_repo_path_from_workspace(&workspace_path).unwrap_or_else(|| workspace_path.clone());

  for submodule in &listed {
    if let Some(only) = path {
      if submodule.path != only {
        continue;
      }
    }
    if submodule.pin.is_empty() {
      return Err(format!(
        "Submodule '{}' has no gitlink pin in the current commit",
        submodule.path
      ));
    }
    populate_submodule(&git_repo_path, &workspace_path, submodule)?;
  }

  list_submodules(repo_path, workspace_id)
}

fn resolve_workspace_dir(repo_path: &str, workspace_id: Option<i64>) -> Result<String, String> {
  crate::core::resolve_workspace_dir(repo_path, workspace_id)
}

#[derive(Debug, Clone)]
struct SubmoduleSpec {
  name: String,
  path: String,
  url: String,
}

fn parse_gitmodules(contents: &str) -> Vec<SubmoduleSpec> {
  let mut specs: BTreeMap<String, SubmoduleSpec> = BTreeMap::new();
  let mut current: Option<String> = None;

  for raw in contents.lines() {
    let line = raw.trim();
    if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
      continue;
    }
    if let Some(rest) = line.strip_prefix("[submodule ") {
      if let Some(name) = rest.strip_suffix(']').map(unquote_config_value) {
        current = Some(name.clone());
        specs.entry(name.clone()).or_insert(SubmoduleSpec {
          name,
          path: String::new(),
          url: String::new(),
        });
      }
      continue;
    }
    let Some(name) = current.as_ref() else {
      continue;
    };
    let Some((key, value)) = line.split_once('=') else {
      continue;
    };
    let key = key.trim();
    let value = unquote_config_value(value.trim());
    if let Some(spec) = specs.get_mut(name) {
      match key {
        "path" => spec.path = value,
        "url" => spec.url = value,
        _ => {}
      }
    }
  }

  specs
    .into_values()
    .filter(|spec| !spec.path.is_empty() && !spec.url.is_empty())
    .collect()
}

fn unquote_config_value(value: &str) -> String {
  let trimmed = value.trim();
  ['"', '\'']
    .into_iter()
    .find_map(|quote| {
      trimmed
        .strip_prefix(quote)
        .and_then(|inner| inner.strip_suffix(quote))
    })
    .unwrap_or(trimmed)
    .to_string()
}

fn gitlinks_for_workspace(workspace_path: &str, git_repo_path: &str) -> BTreeMap<String, String> {
  if let Some(commit_hex) = working_copy_commit_hex(workspace_path) {
    let pins = gitlinks_from_rev(git_repo_path, &commit_hex);
    if !pins.is_empty() {
      return pins;
    }
  }
  gitlinks_from_rev(git_repo_path, "HEAD")
}

fn working_copy_commit_hex(workspace_path: &str) -> Option<String> {
  jj::jj_working_copy_commit_hex(workspace_path)
}

fn gitlinks_from_rev(git_repo_path: &str, rev: &str) -> BTreeMap<String, String> {
  let Ok(repo) = gix::open(git_repo_path) else {
    return BTreeMap::new();
  };
  let Some(tree) = repo
    .rev_parse_single(rev)
    .ok()
    .and_then(|id| id.object().ok())
    .and_then(|object| object.peel_to_tree().ok())
  else {
    return BTreeMap::new();
  };
  let mut recorder = gix::traverse::tree::Recorder::default();
  if tree.traverse().breadthfirst(&mut recorder).is_err() {
    return BTreeMap::new();
  }
  recorder
    .records
    .into_iter()
    .filter(|entry| entry.mode.is_commit())
    .map(|entry| (entry.filepath.to_string(), entry.oid.to_string()))
    .collect()
}

fn inspect_submodule(workspace_root: &Path, spec: SubmoduleSpec, pin: String) -> GitSubmodule {
  let checkout = workspace_root.join(&spec.path);
  if !submodule_git_present(&checkout) {
    return GitSubmodule {
      name: spec.name,
      path: spec.path,
      url: spec.url,
      pin,
      head: None,
      state: SubmoduleState::Missing,
    };
  }

  let head = nested_head_hex(&checkout);
  let dirty = nested_is_dirty(&checkout);
  let state = match &head {
    Some(head) if !pin.is_empty() && !ids_match(head, &pin) => SubmoduleState::Diverged,
    Some(_) if dirty => SubmoduleState::Dirty,
    Some(_) => SubmoduleState::Clean,
    None => SubmoduleState::Missing,
  };

  GitSubmodule {
    name: spec.name,
    path: spec.path,
    url: spec.url,
    pin,
    head,
    state,
  }
}

fn submodule_git_present(checkout: &Path) -> bool {
  let marker = checkout.join(".git");
  marker.is_file() || marker.is_dir()
}

fn nested_head_hex(checkout: &Path) -> Option<String> {
  let repo = gix::open(checkout).ok()?;
  let id = repo.head_id().ok()?;
  Some(id.to_string())
}

fn nested_is_dirty(checkout: &Path) -> bool {
  let Ok(repo) = gix::open(checkout) else {
    return false;
  };
  let Ok(status) = repo.status(gix::progress::Discard) else {
    return false;
  };
  let Ok(mut changes) = status.into_iter(Vec::<gix::bstr::BString>::new()) else {
    return false;
  };
  changes.any(|change| change.is_ok())
}

fn ids_match(left: &str, right: &str) -> bool {
  let a = left.trim().to_ascii_lowercase();
  let b = right.trim().to_ascii_lowercase();
  a == b || a.starts_with(&b) || b.starts_with(&a)
}

fn populate_submodule(
  git_repo_path: &str,
  workspace_path: &str,
  submodule: &GitSubmodule,
) -> Result<(), String> {
  let modules_dir = Path::new(git_repo_path)
    .join(".git")
    .join("modules")
    .join(&submodule.name);
  if let Some(parent) = modules_dir.parent() {
    fs::create_dir_all(parent).map_err(|e| format!("Failed to create modules dir: {e}"))?;
  }

  let pin = gix::ObjectId::from_hex(submodule.pin.as_bytes())
    .map_err(|e| format!("Invalid pin '{}': {e}", submodule.pin))?;
  if !modules_dir.join("HEAD").is_file() && !modules_dir.join("objects").is_dir() {
    if modules_dir.exists() {
      fs::remove_dir_all(&modules_dir)
        .map_err(|e| format!("Failed to clear incomplete submodule store: {e}"))?;
    }
    clone_submodule_store(&submodule.url, &modules_dir)?;
  }
  let repo = gix::open(&modules_dir)
    .map_err(|e| format!("Failed to open submodule '{}': {e}", submodule.path))?;
  if !repo.has_object(pin) {
    fetch_all_remotes(&repo);
  }

  let checkout = Path::new(workspace_path).join(&submodule.path);
  fs::create_dir_all(&checkout).map_err(|e| format!("Failed to create submodule checkout: {e}"))?;
  write_gitfile(&checkout, &modules_dir)?;
  checkout_detached(&repo, &checkout, pin).map_err(|e| {
    format!(
      "Failed to check out submodule '{}' at {}: {e}",
      submodule.path, pin
    )
  })
}

/// Clones `url` into a git dir with no worktree of its own; each checkout
/// reaches it through a `.git` file.
fn clone_submodule_store(url: &str, modules_dir: &Path) -> Result<(), String> {
  let clone_err = |e: &dyn std::fmt::Display| format!("Failed to clone submodule {url}: {e}");
  let mut prepare = gix::prepare_clone_bare(url, modules_dir).map_err(|e| clone_err(&e))?;
  prepare
    .fetch_only(gix::progress::Discard, &AtomicBool::new(false))
    .map_err(|e| clone_err(&e))?;
  // Not bare, so git and gix treat the directory holding the `.git` file as its worktree.
  let config_path = modules_dir.join("config");
  let mut config =
    gix::config::File::from_path_no_includes(config_path.clone(), gix::config::Source::Local)
      .map_err(|e| clone_err(&e))?;
  config
    .set_raw_value(&"core.bare", "false")
    .map_err(|e| clone_err(&e))?;
  fs::write(&config_path, config.to_bstring()).map_err(|e| clone_err(&e))
}

/// Best effort, like `git fetch --all`: a pin that is still missing afterwards
/// fails the checkout with a clear error.
fn fetch_all_remotes(repo: &gix::Repository) {
  let interrupt = AtomicBool::new(false);
  for name in repo.remote_names() {
    let Ok(remote) = repo.find_remote(name.as_ref()) else {
      continue;
    };
    let fetched = remote
      .connect(gix::remote::Direction::Fetch)
      .map_err(|e| e.to_string())
      .and_then(|connection| {
        connection
          .prepare_fetch(gix::progress::Discard, Default::default())
          .map_err(|e| e.to_string())
      })
      .and_then(|prepare| {
        prepare
          .receive(gix::progress::Discard, &interrupt)
          .map_err(|e| e.to_string())
      });
    if let Err(e) = fetched {
      tracing::warn!("Failed to fetch submodule remote '{}': {}", name, e);
    }
  }
}

/// `git checkout --detach --force <pin>`: writes the pin's tree over the
/// checkout, deletes files only the previous index tracked, and leaves
/// untracked files alone.
fn checkout_detached(
  repo: &gix::Repository,
  checkout: &Path,
  pin: gix::ObjectId,
) -> Result<(), String> {
  let tree_id = repo
    .find_commit(pin)
    .map_err(|e| e.to_string())?
    .tree_id()
    .map_err(|e| e.to_string())?
    .detach();
  let state = gix::index::State::from_tree(&tree_id, &repo.objects, Default::default())
    .map_err(|e| e.to_string())?;
  let mut index = gix::index::File::from_state(state, repo.index_path());

  let previous = repo.index_or_empty().map_err(|e| e.to_string())?;
  let kept: HashSet<&BStr> = index.entries().iter().map(|e| e.path(&index)).collect();
  for entry in previous.entries() {
    let path = entry.path(&previous);
    if !kept.contains(path) {
      if let Ok(relative) = path.to_path() {
        fs::remove_file(checkout.join(relative)).best_effort();
      }
    }
  }

  let mut options = repo
    .checkout_options(gix::worktree::stack::state::attributes::Source::IdMapping)
    .map_err(|e| e.to_string())?;
  options.overwrite_existing = true;
  options.destination_is_initially_empty = false;
  let objects = repo.objects.clone().into_arc().map_err(|e| e.to_string())?;
  let outcome = gix::worktree::state::checkout(
    &mut index,
    checkout,
    objects,
    &gix::progress::Discard,
    &gix::progress::Discard,
    &AtomicBool::new(false),
    options,
  )
  .map_err(|e| e.to_string())?;
  if let Some(failed) = outcome.errors.first() {
    return Err(format!("{}: {}", failed.path, failed.error));
  }
  index.write(Default::default()).map_err(|e| e.to_string())?;

  // The HEAD reflog entry needs a committer; like git, fall back to a
  // generated identity when none is configured.
  let mut repo = repo.clone();
  repo
    .committer_or_set_generic_fallback()
    .map_err(|e| e.to_string())?;
  repo
    .edit_reference(RefEdit {
      change: Change::Update {
        log: LogChange::default(),
        expected: PreviousValue::Any,
        new: Target::Object(pin),
      },
      name: "HEAD".try_into().map_err(|e| format!("{e}"))?,
      deref: false,
    })
    .map_err(|e| e.to_string())?;
  Ok(())
}

fn write_gitfile(checkout: &Path, modules_dir: &Path) -> Result<(), String> {
  let gitfile = checkout.join(".git");
  if gitfile.is_dir() {
    return Ok(());
  }
  fs::write(&gitfile, format!("gitdir: {}\n", modules_dir.display()))
    .map_err(|e| format!("Failed to write gitfile: {e}"))
}

const SUBMODULE_SYNC_KEY: &str = "submodule_sync";

fn load_submodule_sync_map(repo_path: &str) -> BTreeMap<String, bool> {
  let app_db_path = crate::core::resolve_app_db_path(repo_path);
  if !app_db_path.exists() {
    return BTreeMap::new();
  }
  let Ok(db) = crate::db::Database::new(app_db_path) else {
    return BTreeMap::new();
  };
  let Ok(Some(raw)) = db.get_repo_setting(repo_path, SUBMODULE_SYNC_KEY) else {
    return BTreeMap::new();
  };
  serde_json::from_str(&raw).unwrap_or_default()
}

fn save_submodule_sync_map(repo_path: &str, map: &BTreeMap<String, bool>) -> Result<(), String> {
  let app_db_path = crate::core::resolve_app_db_path(repo_path);
  let db = crate::db::Database::new(app_db_path).map_err(|e| e.to_string())?;
  db.init().map_err(|e| e.to_string())?;
  let raw = serde_json::to_string(map).map_err(|e| e.to_string())?;
  db.set_repo_setting(repo_path, SUBMODULE_SYNC_KEY, &raw)
    .map_err(|e| e.to_string())
}

/// Persist whether `path` should be populated in every workspace, then apply it.
///
/// Default is off. Enabling clones/checks out the pin in the home repo and all
/// registered workspaces. Disabling only clears the preference.
pub fn set_submodule_synced(
  repo_path: &str,
  path: &str,
  enabled: bool,
) -> Result<Vec<GitSubmodule>, String> {
  let mut map = load_submodule_sync_map(repo_path);
  if enabled {
    map.insert(path.to_string(), true);
  } else {
    map.remove(path);
  }
  save_submodule_sync_map(repo_path, &map)?;
  if enabled {
    populate_submodule_everywhere(repo_path, path)?;
  }
  list_submodules(repo_path, None)
}

/// Populate every submodule marked synced into one working copy.
pub fn populate_synced_submodules(
  repo_path: &str,
  workspace_id: Option<i64>,
) -> Result<(), String> {
  for (path, enabled) in load_submodule_sync_map(repo_path) {
    if enabled {
      update_submodules(repo_path, workspace_id, Some(&path))?;
    }
  }
  Ok(())
}

fn populate_submodule_everywhere(repo_path: &str, path: &str) -> Result<(), String> {
  update_submodules(repo_path, None, Some(path))?;
  if let Ok(workspaces) = crate::core::list_workspaces(repo_path) {
    for workspace in workspaces {
      update_submodules(repo_path, Some(workspace.id), Some(path))?;
    }
  }
  Ok(())
}

/// Inject gitlink rows into a depth-1 directory listing, using the full
/// submodule path as `name` even when the checkout is nested.
pub fn merge_submodules_into_entries(
  repo_path: &str,
  workspace_id: Option<i64>,
  workspace_root: &str,
  entries: &mut Vec<crate::core::WorkspaceEntry>,
) {
  let Ok(listed) = list_submodules(repo_path, workspace_id) else {
    return;
  };
  if listed.is_empty() {
    return;
  }
  let sync = load_submodule_sync_map(repo_path);
  let base = Path::new(workspace_root);
  for submodule in listed {
    let entry_path = base.join(&submodule.path);
    let modified_at = fs::metadata(&entry_path)
      .ok()
      .and_then(|metadata| metadata.modified().ok())
      .map(|modified| chrono::DateTime::<chrono::Utc>::from(modified).to_rfc3339());
    let entry = crate::core::WorkspaceEntry {
      name: submodule.path.clone(),
      path: entry_path.to_string_lossy().into_owned(),
      is_directory: true,
      modified_at,
      submodule_pin: Some(submodule.pin),
      submodule_synced: Some(sync.get(&submodule.path).copied().unwrap_or(false)),
      status: None,
    };
    if let Some(index) = entries
      .iter()
      .position(|existing| existing.name == submodule.path)
    {
      entries[index] = entry;
    } else {
      entries.push(entry);
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::process::Command;
  use tempfile::TempDir;

  fn run(cwd: &Path, program: &str, args: &[&str]) {
    let output = Command::new(program)
      .current_dir(cwd)
      .args(args)
      .output()
      .unwrap_or_else(|e| panic!("{program} {:?}: {e}", args));
    assert!(
      output.status.success(),
      "{program} {:?} failed: {}{}",
      args,
      String::from_utf8_lossy(&output.stdout),
      String::from_utf8_lossy(&output.stderr)
    );
  }

  fn git(cwd: &Path, args: &[&str]) {
    run(cwd, "git", args);
  }

  fn git_allow_file(cwd: &Path, args: &[&str]) {
    let output = Command::new("git")
      .current_dir(cwd)
      .args(["-c", "protocol.file.allow=always"])
      .args(args)
      .output()
      .unwrap_or_else(|e| panic!("git {:?}: {e}", args));
    assert!(
      output.status.success(),
      "git {:?} failed: {}{}",
      args,
      String::from_utf8_lossy(&output.stdout),
      String::from_utf8_lossy(&output.stderr)
    );
  }

  fn init_git_repo(dir: &Path) {
    git(dir, &["init"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "user.name", "Test User"]);
    fs::write(dir.join("README.md"), "sub\n").unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-m", "init"]);
  }

  fn create_superproject_with_submodule() -> (TempDir, String, String) {
    let temp = TempDir::new().unwrap();
    let sub = temp.path().join("sub");
    let superproject = temp.path().join("super");
    fs::create_dir_all(&sub).unwrap();
    fs::create_dir_all(&superproject).unwrap();
    init_git_repo(&sub);
    init_git_repo(&superproject);

    git_allow_file(
      &superproject,
      &["submodule", "add", sub.to_str().unwrap(), "vendor/lib"],
    );
    git(&superproject, &["commit", "-m", "add submodule"]);
    crate::core::init(superproject.to_str().unwrap()).unwrap();

    (
      temp,
      superproject.to_string_lossy().into_owned(),
      sub.to_string_lossy().into_owned(),
    )
  }

  #[test]
  fn parse_gitmodules_reads_path_and_url() {
    let parsed = parse_gitmodules(
      r#"
[submodule "vendor/lib"]
	path = vendor/lib
	url = https://example.com/lib.git
"#,
    );
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].name, "vendor/lib");
    assert_eq!(parsed[0].path, "vendor/lib");
    assert_eq!(parsed[0].url, "https://example.com/lib.git");
  }

  #[test]
  fn list_submodules_returns_empty_when_path_missing() {
    let listed = list_submodules("/tmp/treq-does-not-exist-submodules", None).unwrap();
    assert!(listed.is_empty());
  }

  #[test]
  fn list_submodules_returns_empty_without_gitmodules() {
    let temp = TempDir::new().unwrap();
    fs::write(temp.path().join("README"), "x\n").unwrap();
    let listed = list_submodules(temp.path().to_str().unwrap(), None).unwrap();
    assert!(listed.is_empty());
  }

  #[test]
  fn list_submodules_reports_clean_populated_checkout() {
    let (_temp, superproject, _) = create_superproject_with_submodule();
    let listed = list_submodules(&superproject, None).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].path, "vendor/lib");
    assert!(!listed[0].pin.is_empty());
    assert_eq!(listed[0].state, SubmoduleState::Clean);
    assert!(listed[0].head.is_some());
  }

  #[test]
  fn list_submodules_reports_missing_after_checkout_removed() {
    let (_temp, superproject, _) = create_superproject_with_submodule();
    fs::remove_dir_all(Path::new(&superproject).join("vendor/lib")).unwrap();
    let listed = list_submodules(&superproject, None).unwrap();
    assert_eq!(listed[0].state, SubmoduleState::Missing);
    assert!(listed[0].head.is_none());
  }

  #[test]
  fn update_submodules_restores_missing_checkout_to_pin() {
    let (_temp, superproject, _) = create_superproject_with_submodule();
    fs::remove_dir_all(Path::new(&superproject).join("vendor/lib")).unwrap();
    let updated = update_submodules(&superproject, None, None).unwrap();
    assert_eq!(updated.len(), 1);
    assert_eq!(updated[0].state, SubmoduleState::Clean);
    assert!(Path::new(&superproject).join("vendor/lib/.git").exists());
    let head = updated[0].head.as_deref().unwrap();
    assert!(ids_match(head, &updated[0].pin));
  }

  fn git_stdout(cwd: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
      .current_dir(cwd)
      .args(args)
      .output()
      .unwrap();
    assert!(output.status.success(), "git {:?}", args);
    String::from_utf8_lossy(&output.stdout).trim().to_string()
  }

  #[test]
  fn update_submodules_clones_missing_store_into_usable_checkout() {
    let (_temp, superproject, _) = create_superproject_with_submodule();
    let root = Path::new(&superproject);
    fs::remove_dir_all(root.join(".git/modules/vendor/lib")).unwrap();
    fs::remove_dir_all(root.join("vendor/lib")).unwrap();

    let updated = update_submodules(&superproject, None, None).unwrap();

    assert_eq!(updated[0].state, SubmoduleState::Clean);
    let checkout = root.join("vendor/lib");
    assert_eq!(
      fs::read_to_string(checkout.join("README.md")).unwrap(),
      "sub\n"
    );
    // Plain git sees a detached, clean worktree at the pin.
    assert_eq!(
      git_stdout(&checkout, &["rev-parse", "HEAD"]),
      updated[0].pin
    );
    assert_eq!(git_stdout(&checkout, &["status", "--porcelain"]), "");
  }

  #[test]
  fn update_submodules_fetches_pin_missing_from_store() {
    let (_temp, superproject, sub) = create_superproject_with_submodule();
    let root = Path::new(&superproject);
    let store = root.join(".git/modules/vendor/lib");
    fs::remove_dir_all(&store).unwrap();
    fs::remove_dir_all(root.join("vendor/lib")).unwrap();
    git(root, &["init", "--bare", store.to_str().unwrap()]);
    git(&store, &["remote", "add", "origin", &sub]);

    let updated = update_submodules(&superproject, None, None).unwrap();

    assert_eq!(updated[0].state, SubmoduleState::Clean);
    assert!(ids_match(
      updated[0].head.as_deref().unwrap(),
      &updated[0].pin
    ));
  }

  #[test]
  fn update_submodules_restores_pin_and_keeps_untracked_files() {
    let (_temp, superproject, sub) = create_superproject_with_submodule();
    let sub = Path::new(&sub);
    fs::write(sub.join("NEW.md"), "new\n").unwrap();
    git(sub, &["rm", "-q", "README.md"]);
    git(sub, &["add", "NEW.md"]);
    git(sub, &["commit", "-m", "replace readme"]);
    let checkout = Path::new(&superproject).join("vendor/lib");
    git(&checkout, &["fetch", "-q", "origin"]);
    git(&checkout, &["checkout", "-q", "--detach", "FETCH_HEAD"]);
    fs::write(checkout.join("keep.txt"), "mine\n").unwrap();
    assert_eq!(
      list_submodules(&superproject, None).unwrap()[0].state,
      SubmoduleState::Diverged
    );

    let updated = update_submodules(&superproject, None, None).unwrap();

    assert!(ids_match(
      updated[0].head.as_deref().unwrap(),
      &updated[0].pin
    ));
    assert_eq!(
      fs::read_to_string(checkout.join("README.md")).unwrap(),
      "sub\n"
    );
    assert!(!checkout.join("NEW.md").exists());
    assert_eq!(
      fs::read_to_string(checkout.join("keep.txt")).unwrap(),
      "mine\n"
    );
    assert_eq!(
      git_stdout(&checkout, &["status", "--porcelain"]),
      "?? keep.txt"
    );
  }

  #[test]
  fn list_submodules_reports_dirty_when_nested_files_change() {
    let (_temp, superproject, _) = create_superproject_with_submodule();
    fs::write(
      Path::new(&superproject).join("vendor/lib/README.md"),
      "dirty\n",
    )
    .unwrap();
    let listed = list_submodules(&superproject, None).unwrap();
    assert_eq!(listed[0].state, SubmoduleState::Dirty);
  }

  #[test]
  fn ls_workspace_lists_nested_submodule_path_with_pin_unsynced() {
    let (_temp, superproject, _) = create_superproject_with_submodule();
    let entries = crate::core::ls_workspace(&superproject, None).unwrap();
    let row = entries
      .iter()
      .find(|entry| entry.name == "vendor/lib")
      .expect("nested submodule path should appear in the directory list");
    assert!(row.is_directory);
    assert!(row
      .submodule_pin
      .as_ref()
      .is_some_and(|pin| !pin.is_empty()));
    assert_eq!(row.submodule_synced, Some(false));
  }

  #[test]
  fn set_submodule_synced_defaults_off_and_populates_when_enabled() {
    let (_temp, superproject, _) = create_superproject_with_submodule();
    fs::remove_dir_all(Path::new(&superproject).join("vendor/lib")).unwrap();

    let listed = crate::core::ls_workspace(&superproject, None).unwrap();
    let row = listed
      .iter()
      .find(|entry| entry.name == "vendor/lib")
      .unwrap();
    assert_eq!(row.submodule_synced, Some(false));

    set_submodule_synced(&superproject, "vendor/lib", true).unwrap();
    assert!(Path::new(&superproject).join("vendor/lib/.git").exists());
    let listed = crate::core::ls_workspace(&superproject, None).unwrap();
    let row = listed
      .iter()
      .find(|entry| entry.name == "vendor/lib")
      .unwrap();
    assert_eq!(row.submodule_synced, Some(true));
  }

  #[test]
  fn set_submodule_synced_populates_existing_and_new_workspaces() {
    let (_temp, superproject, _) = create_superproject_with_submodule();
    let workspace =
      crate::core::create_workspace(&superproject, "feat/sub-sync", None, None, None, None, None)
        .unwrap();
    let workspace_root = Path::new(&superproject)
      .join(".treq")
      .join("workspaces")
      .join(&workspace.workspace_path);
    fs::remove_dir_all(workspace_root.join("vendor/lib")).ok();
    fs::remove_dir_all(Path::new(&superproject).join("vendor/lib")).ok();

    set_submodule_synced(&superproject, "vendor/lib", true).unwrap();
    assert!(workspace_root.join("vendor/lib/.git").exists());

    let second = crate::core::create_workspace(
      &superproject,
      "feat/sub-sync-2",
      None,
      None,
      None,
      None,
      None,
    )
    .unwrap();
    let second_root = Path::new(&superproject)
      .join(".treq")
      .join("workspaces")
      .join(&second.workspace_path);
    assert!(second_root.join("vendor/lib/.git").exists());
  }
}
