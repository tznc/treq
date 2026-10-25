use crate::result_ext::ResultExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

pub const PROJECT_SKILL_MARKER: &str = ".treq-generated";
pub const DEFAULT_SKILLS_CATALOG_URL: &str = "https://treq.dev/skills/catalog.json";
const INDEX_FILE: &str = "index.json";
const APP_SETTING_KEY: &str = "installed_skills";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SkillInstallScope {
  Application,
  Repository,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillFile {
  pub path: String,
  #[serde(default)]
  pub size: u64,
  #[serde(default)]
  pub binary: bool,
  #[serde(default)]
  pub github_url: Option<String>,
  #[serde(default)]
  pub raw_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SkillCatalogEntry {
  pub id: String,
  pub name: String,
  #[serde(default)]
  pub description: Option<String>,
  pub source: String,
  #[serde(default)]
  pub category: Option<String>,
  #[serde(default)]
  pub license: Option<String>,
  #[serde(default)]
  pub proprietary: bool,
  #[serde(default)]
  pub url: Option<String>,
  /// Path of the skill's page on the treq.dev skill registry.
  #[serde(default)]
  pub route: Option<String>,
  #[serde(default)]
  pub checksum: Option<String>,
  #[serde(default)]
  pub files: Vec<SkillFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SkillCatalog {
  #[serde(default)]
  pub generated_at: Option<String>,
  #[serde(default)]
  pub sources: Vec<serde_json::Value>,
  #[serde(default)]
  pub skills: Vec<SkillCatalogEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstalledSkill {
  pub id: String,
  pub name: String,
  pub checksum: String,
  pub scope: SkillInstallScope,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SkillCatalogSkillView {
  #[serde(flatten)]
  pub skill: SkillCatalogEntry,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub installed: Option<InstalledSkill>,
  /// The repository holds a user-owned copy, which Treq never overwrites.
  #[serde(default)]
  pub customized: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CustomizedSkill {
  pub id: String,
  pub name: String,
  /// Relative to the repository root.
  pub agents_skill_path: String,
  pub claude_skill_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SkillCatalogView {
  #[serde(default)]
  pub generated_at: Option<String>,
  #[serde(default)]
  pub sources: Vec<serde_json::Value>,
  pub skills: Vec<SkillCatalogSkillView>,
}

pub fn skill_checksum(files: &[(String, Vec<u8>)]) -> String {
  let mut items: Vec<&(String, Vec<u8>)> = files.iter().collect();
  items.sort_by(|a, b| a.0.cmp(&b.0));
  let mut hasher = Sha256::new();
  for (path, bytes) in items {
    hasher.update(path.as_bytes());
    hasher.update([0u8]);
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
  }
  hasher
    .finalize()
    .iter()
    .map(|byte| format!("{byte:02x}"))
    .collect()
}

pub fn parse_catalog(bytes: &[u8]) -> Result<SkillCatalog, String> {
  serde_json::from_slice(bytes).map_err(|e| format!("Failed to parse skills catalog: {e}"))
}

pub fn catalog_url(override_url: Option<&str>) -> String {
  if let Some(url) = override_url.map(str::trim).filter(|s| !s.is_empty()) {
    return url.to_string();
  }
  std::env::var("TREQ_SKILLS_CATALOG_URL")
    .ok()
    .map(|s| s.trim().to_string())
    .filter(|s| !s.is_empty())
    .unwrap_or_else(|| DEFAULT_SKILLS_CATALOG_URL.to_string())
}

pub fn read_bytes_from_locator(locator: &str) -> Result<Vec<u8>, String> {
  if let Some(path) = locator.strip_prefix("file://") {
    return fs::read(path).map_err(|e| format!("Failed to read {path}: {e}"));
  }
  if !locator.contains("://") {
    return fs::read(locator).map_err(|e| format!("Failed to read {locator}: {e}"));
  }
  Err(format!("Unsupported locator (use HTTP fetch): {locator}"))
}

pub fn app_skills_root() -> Result<PathBuf, String> {
  let dir =
    crate::core::app_data_dir().ok_or_else(|| "TREQ_APP_DATA_DIR is not set".to_string())?;
  Ok(dir.join("skills"))
}

pub fn repo_skills_root(repo_path: &str) -> PathBuf {
  Path::new(repo_path).join(".treq").join("skills")
}

fn pack_dir(root: &Path, skill_id: &str) -> Result<PathBuf, String> {
  Ok(root.join(sanitize_id(skill_id)?))
}

fn sanitize_id(skill_id: &str) -> Result<String, String> {
  let trimmed = skill_id.trim();
  if trimmed.is_empty() {
    return Err("Skill id is required".to_string());
  }
  if trimmed.contains("..") || trimmed.starts_with('/') || trimmed.contains('\\') {
    return Err("Invalid skill id".to_string());
  }
  Ok(trimmed.replace('/', "__"))
}

pub fn skill_dirname(name: &str) -> Result<String, String> {
  let trimmed = name.trim();
  if trimmed.is_empty() {
    return Err("Skill name is required".to_string());
  }
  if trimmed.contains("..") || trimmed.contains('/') || trimmed.contains('\\') {
    return Err("Invalid skill name".to_string());
  }
  Ok(trimmed.to_string())
}

fn load_index(root: &Path) -> Result<Vec<InstalledSkill>, String> {
  let path = root.join(INDEX_FILE);
  if !path.exists() {
    return Ok(Vec::new());
  }
  let bytes = fs::read(&path).map_err(|e| format!("Failed to read skills index: {e}"))?;
  serde_json::from_slice(&bytes).map_err(|e| format!("Failed to parse skills index: {e}"))
}

fn save_index(root: &Path, skills: &[InstalledSkill]) -> Result<(), String> {
  fs::create_dir_all(root).map_err(|e| format!("Failed to create skills dir: {e}"))?;
  let path = root.join(INDEX_FILE);
  let json = serde_json::to_string_pretty(skills)
    .map_err(|e| format!("Failed to serialize skills index: {e}"))?;
  fs::write(path, json).map_err(|e| format!("Failed to write skills index: {e}"))
}

pub fn list_installed_skills(repo_path: Option<&str>) -> Result<Vec<InstalledSkill>, String> {
  let mut out = Vec::new();
  if let Ok(root) = app_skills_root() {
    out.extend(load_index(&root)?);
  }
  if let Some(repo) = repo_path.filter(|p| !p.trim().is_empty()) {
    for skill in load_index(&repo_skills_root(repo))? {
      out.retain(|existing| existing.id != skill.id);
      out.push(skill);
    }
  }
  out.sort_by(|a, b| a.id.cmp(&b.id));
  Ok(out)
}

pub fn merge_catalog_with_installed(
  catalog: SkillCatalog,
  repo_path: Option<&str>,
) -> Result<SkillCatalogView, String> {
  let installed = list_installed_skills(repo_path)?;
  let skills = catalog
    .skills
    .into_iter()
    .map(|skill| {
      let installed = installed.iter().find(|item| item.id == skill.id).cloned();
      let customized = installed.is_some()
        && repo_path
          .filter(|p| !p.trim().is_empty())
          .is_some_and(|repo| has_user_owned_copy(Path::new(repo), &skill.name));
      SkillCatalogSkillView {
        skill,
        installed,
        customized,
      }
    })
    .collect();
  Ok(SkillCatalogView {
    generated_at: catalog.generated_at,
    sources: catalog.sources,
    skills,
  })
}

fn assert_safe_relative_path(path: &str) -> Result<(), String> {
  let normalized = path.replace('\\', "/");
  if normalized.is_empty()
    || Path::new(&normalized).is_absolute()
    || normalized.split('/').any(|part| part == "..")
  {
    return Err(format!("Unsafe skill file path: {path}"));
  }
  Ok(())
}

/// Builds a directory in a sibling staging dir, then swaps it into `dest`:
/// the old directory is moved aside, the new one renamed in, and the old one
/// removed. If `build` or a rename fails, the old directory is left in place.
fn replace_dir(dest: &Path, build: impl FnOnce(&Path) -> Result<(), String>) -> Result<(), String> {
  let parent = dest
    .parent()
    .ok_or_else(|| "Invalid skill pack path".to_string())?;
  fs::create_dir_all(parent).map_err(|e| format!("Failed to create skills dir: {e}"))?;
  let name = dest
    .file_name()
    .ok_or_else(|| "Invalid skill pack path".to_string())?
    .to_string_lossy();
  let staging = parent.join(format!(".{name}.new-{}", std::process::id()));
  let backup = parent.join(format!(".{name}.old-{}", std::process::id()));
  fs::remove_dir_all(&staging).best_effort();
  fs::remove_dir_all(&backup).best_effort();
  if let Err(err) = fs::create_dir_all(&staging)
    .map_err(|e| format!("Failed to create skill pack dir: {e}"))
    .and_then(|()| build(&staging))
  {
    fs::remove_dir_all(&staging).best_effort();
    return Err(err);
  }
  let had_old = dest.exists();
  if had_old {
    if let Err(e) = fs::rename(dest, &backup) {
      fs::remove_dir_all(&staging).best_effort();
      return Err(format!("Failed to replace skill pack: {e}"));
    }
  }
  if let Err(e) = fs::rename(&staging, dest) {
    if had_old {
      fs::rename(&backup, dest).warn_on_err("restore previous skill pack");
    }
    fs::remove_dir_all(&staging).best_effort();
    return Err(format!("Failed to replace skill pack: {e}"));
  }
  if had_old {
    fs::remove_dir_all(&backup).best_effort();
  }
  Ok(())
}

pub fn write_skill_pack(dest: &Path, files: &[(String, Vec<u8>)]) -> Result<(), String> {
  replace_dir(dest, |staging| {
    for (relative, bytes) in files {
      assert_safe_relative_path(relative)?;
      let path = staging.join(relative);
      let parent = path
        .parent()
        .ok_or_else(|| format!("Invalid skill file path: {relative}"))?;
      fs::create_dir_all(parent).map_err(|e| format!("Failed to create skill file dir: {e}"))?;
      fs::write(&path, bytes).map_err(|e| format!("Failed to write skill file: {e}"))?;
    }
    Ok(())
  })
}

/// The files' checksum, checked against the catalog's when it lists one.
pub fn verify_skill_checksum(
  entry: &SkillCatalogEntry,
  files: &[(String, Vec<u8>)],
) -> Result<String, String> {
  let computed = skill_checksum(files);
  if let Some(expected) = entry
    .checksum
    .as_deref()
    .map(str::trim)
    .filter(|s| !s.is_empty())
  {
    if expected != computed {
      return Err(format!(
        "Checksum mismatch for skill '{}': catalog {expected}, downloaded {computed}",
        entry.id
      ));
    }
  }
  Ok(computed)
}

pub fn install_skill_files(
  entry: &SkillCatalogEntry,
  files: &[(String, Vec<u8>)],
  scope: SkillInstallScope,
  repo_path: Option<&str>,
) -> Result<InstalledSkill, String> {
  if entry.proprietary {
    return Err("Proprietary skills cannot be installed from the catalog".to_string());
  }
  if files.is_empty() {
    return Err("Skill has no files to install".to_string());
  }
  let computed = verify_skill_checksum(entry, files)?;

  let root = match scope {
    SkillInstallScope::Application => app_skills_root()?,
    SkillInstallScope::Repository => {
      let repo = repo_path
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Repository path is required for repo-level install".to_string())?;
      repo_skills_root(repo)
    }
  };
  let dest = pack_dir(&root, &entry.id)?;
  write_skill_pack(&dest, files)?;

  let record = InstalledSkill {
    id: entry.id.clone(),
    name: entry.name.clone(),
    checksum: computed,
    scope,
  };
  let mut index = load_index(&root)?;
  index.retain(|item| item.id != record.id);
  index.push(record.clone());
  index.sort_by(|a, b| a.id.cmp(&b.id));
  save_index(&root, &index)?;

  // Installing at one scope removes the other so a skill has a single level.
  let other_root = match scope {
    SkillInstallScope::Application => repo_path
      .map(str::trim)
      .filter(|s| !s.is_empty())
      .map(repo_skills_root),
    SkillInstallScope::Repository => app_skills_root().ok(),
  };
  if let Some(other) = other_root {
    uninstall_from_root(&other, &entry.id).warn_on_err("uninstall from root");
  }

  Ok(record)
}

fn uninstall_from_root(root: &Path, skill_id: &str) -> Result<bool, String> {
  let mut index = load_index(root)?;
  let before = index.len();
  index.retain(|item| item.id != skill_id);
  if index.len() == before {
    return Ok(false);
  }
  if let Ok(dir) = pack_dir(root, skill_id) {
    if dir.exists() {
      fs::remove_dir_all(&dir).map_err(|e| format!("Failed to remove skill pack: {e}"))?;
    }
  }
  save_index(root, &index)?;
  Ok(true)
}

pub fn uninstall_skill(skill_id: &str, repo_path: Option<&str>) -> Result<(), String> {
  let mut removed = false;
  if let Ok(root) = app_skills_root() {
    removed |= uninstall_from_root(&root, skill_id)?;
  }
  if let Some(repo) = repo_path.filter(|p| !p.trim().is_empty()) {
    removed |= uninstall_from_root(&repo_skills_root(repo), skill_id)?;
  }
  if !removed {
    return Err(format!("Skill '{skill_id}' is not installed"));
  }
  Ok(())
}

pub fn set_skill_install_scope(
  skill_id: &str,
  scope: SkillInstallScope,
  repo_path: Option<&str>,
) -> Result<InstalledSkill, String> {
  let installed = list_installed_skills(repo_path)?;
  let current = installed
    .into_iter()
    .find(|item| item.id == skill_id)
    .ok_or_else(|| format!("Skill '{skill_id}' is not installed"))?;
  if current.scope == scope {
    return Ok(current);
  }
  let src_root = match current.scope {
    SkillInstallScope::Application => app_skills_root()?,
    SkillInstallScope::Repository => {
      let repo = repo_path
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Repository path is required".to_string())?;
      repo_skills_root(repo)
    }
  };
  let dest_root = match scope {
    SkillInstallScope::Application => app_skills_root()?,
    SkillInstallScope::Repository => {
      let repo = repo_path
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Repository path is required for repo-level install".to_string())?;
      repo_skills_root(repo)
    }
  };
  let src = pack_dir(&src_root, skill_id)?;
  let dest = pack_dir(&dest_root, skill_id)?;
  if !src.exists() {
    return Err(format!("Installed skill files missing for '{skill_id}'"));
  }
  replace_dir(&dest, |staging| copy_dir_recursive(&src, staging))?;
  uninstall_from_root(&src_root, skill_id)?;
  let record = InstalledSkill {
    id: current.id,
    name: current.name,
    checksum: current.checksum,
    scope,
  };
  let mut index = load_index(&dest_root)?;
  index.retain(|item| item.id != record.id);
  index.push(record.clone());
  save_index(&dest_root, &index)?;
  Ok(record)
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<(), String> {
  fs::create_dir_all(dest).map_err(|e| format!("Failed to create skill pack dir: {e}"))?;
  for entry in fs::read_dir(src).map_err(|e| format!("Failed to read skill pack: {e}"))? {
    let entry = entry.map_err(|e| format!("Failed to read skill pack: {e}"))?;
    let ty = entry
      .file_type()
      .map_err(|e| format!("Failed to read skill pack: {e}"))?;
    let target = dest.join(entry.file_name());
    if ty.is_symlink() {
      // Never follow a link out of the pack. A link to a file that resolves
      // inside `src` is copied as that file; directory links are skipped (they
      // could loop back into the tree).
      let inside = fs::canonicalize(entry.path())
        .ok()
        .zip(fs::canonicalize(src).ok())
        .is_some_and(|(resolved, root)| resolved.starts_with(root) && resolved.is_file());
      if inside {
        fs::copy(entry.path(), target).map_err(|e| format!("Failed to copy skill file: {e}"))?;
      }
    } else if ty.is_dir() {
      copy_dir_recursive(&entry.path(), &target)?;
    } else {
      fs::copy(entry.path(), target).map_err(|e| format!("Failed to copy skill file: {e}"))?;
    }
  }
  Ok(())
}

fn read_pack_files(dir: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
  let mut files = Vec::new();
  collect_files(dir, dir, &mut files)?;
  Ok(files)
}

fn collect_files(base: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) -> Result<(), String> {
  for entry in fs::read_dir(dir).map_err(|e| format!("Failed to read skill pack: {e}"))? {
    let entry = entry.map_err(|e| format!("Failed to read skill pack: {e}"))?;
    let path = entry.path();
    if path.is_dir() {
      collect_files(base, &path, out)?;
    } else {
      let rel = path
        .strip_prefix(base)
        .map_err(|e| format!("Failed to relativize skill file: {e}"))?
        .to_string_lossy()
        .replace('\\', "/");
      let bytes = fs::read(&path).map_err(|e| format!("Failed to read skill file: {e}"))?;
      out.push((rel, bytes));
    }
  }
  Ok(())
}

fn install_generated_skill_copy(
  workspace: &Path,
  relative_parent: &str,
  dirname: &str,
  files: &[(String, Vec<u8>)],
) -> Result<(), String> {
  let skill_dir = workspace.join(relative_parent).join(dirname);
  let marker = skill_dir.join(PROJECT_SKILL_MARKER);
  if skill_dir.exists() && !marker.exists() {
    return Ok(());
  }
  write_skill_pack(&skill_dir, files)?;
  fs::write(&marker, "treq-library\n").map_err(|e| format!("Failed to write skill marker: {e}"))?;
  super::ignore_generated_dir(&skill_dir)
    .map_err(|e| format!("Failed to ignore generated skill: {e}"))
}

const AGENT_SKILL_PARENTS: [&str; 2] = [".agents/skills", ".claude/skills"];

fn is_user_owned_dir(dir: &Path) -> bool {
  dir.exists() && !dir.join(PROJECT_SKILL_MARKER).exists()
}

fn has_user_owned_copy(workspace: &Path, name: &str) -> bool {
  let Ok(dirname) = skill_dirname(name) else {
    return false;
  };
  AGENT_SKILL_PARENTS
    .iter()
    .any(|parent| is_user_owned_dir(&workspace.join(parent).join(&dirname)))
}

/// Write an installed skill into the repository's agent skill folders as a
/// user-owned copy. Without the generated-skill marker,
/// `materialize_installed_skills` leaves these folders alone, so catalog
/// updates never overwrite the user's edits.
pub fn customize_skill(skill_id: &str, repo_path: &str) -> Result<CustomizedSkill, String> {
  let repo = Path::new(repo_path);
  if !repo.exists() {
    return Err(format!("Repository not found: {repo_path}"));
  }
  let record = list_installed_skills(Some(repo_path))?
    .into_iter()
    .find(|item| item.id == skill_id)
    .ok_or_else(|| format!("Skill '{skill_id}' is not installed"))?;
  let root = match record.scope {
    SkillInstallScope::Application => app_skills_root()?,
    SkillInstallScope::Repository => repo_skills_root(repo_path),
  };
  let pack = pack_dir(&root, &record.id)?;
  if !pack.exists() {
    return Err(format!("Installed skill files missing for '{skill_id}'"));
  }
  let files = read_pack_files(&pack)?;
  customize_skill_from_files(&record.id, &record.name, &files, repo_path)
}

/// Writes `files` as a user-owned copy of skill `name` into the repository's
/// agent skill folders: from an installed pack, the registry, or a global
/// skill. Never replaces a copy the user already owns.
pub fn customize_skill_from_files(
  id: &str,
  name: &str,
  files: &[(String, Vec<u8>)],
  repo_path: &str,
) -> Result<CustomizedSkill, String> {
  let repo = Path::new(repo_path);
  if !repo.exists() {
    return Err(format!("Repository not found: {repo_path}"));
  }
  let dirname = skill_dirname(name)?;
  if has_user_owned_copy(repo, name) {
    return Err(format!(
      "Skill '{name}' is already customized in this repository"
    ));
  }
  for parent in AGENT_SKILL_PARENTS {
    write_skill_pack(&repo.join(parent).join(&dirname), files)?;
  }
  Ok(CustomizedSkill {
    id: id.to_string(),
    name: name.to_string(),
    agents_skill_path: format!("{}/{dirname}", AGENT_SKILL_PARENTS[0]),
    claude_skill_path: format!("{}/{dirname}", AGENT_SKILL_PARENTS[1]),
  })
}

/// Copies a skill from the home folder into the repository as a user-owned
/// copy, so agents edit it inside the repository.
pub fn customize_global_skill(
  name: &str,
  source: &Path,
  repo_path: &str,
) -> Result<CustomizedSkill, String> {
  let files = read_pack_files(source)?;
  customize_skill_from_files(name, name, &files, repo_path)
}

static PENDING_SKILL_INSTALL: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Keeps a `treq://skill/install` id that arrived before any window could
/// show it (a link that launched Treq).
pub fn queue_pending_skill_install(skill_id: String) {
  *PENDING_SKILL_INSTALL
    .lock()
    .unwrap_or_else(|p| p.into_inner()) = Some(skill_id);
}

/// The queued link's id, once.
pub fn take_pending_skill_install() -> Option<String> {
  PENDING_SKILL_INSTALL
    .lock()
    .unwrap_or_else(|p| p.into_inner())
    .take()
}

/// Where a user-owned skill lives: this repository or the user's home.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CustomSkillLevel {
  Repo,
  Global,
}

/// A skill folder Treq did not generate: hand-written, or a customized copy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomSkill {
  /// The folder name, which agents use as the skill's name.
  pub name: String,
  /// SKILL.md's frontmatter `description`.
  pub description: Option<String>,
  pub level: CustomSkillLevel,
  /// Each copy's folder (`.agents/skills/<name>`, `.claude/skills/<name>`).
  pub paths: Vec<String>,
}

fn skill_description(skill_md: &Path) -> Option<String> {
  let text = fs::read_to_string(skill_md).ok()?;
  let frontmatter = text.strip_prefix("---")?.split("\n---").next()?;
  frontmatter.lines().find_map(|line| {
    let value = line.trim().strip_prefix("description:")?.trim();
    let value = value.trim_matches(|c| c == '"' || c == '\'');
    (!value.is_empty()).then(|| value.to_string())
  })
}

/// The user-owned skills under `root`'s agent skill folders, by name.
fn custom_skills_in(root: &Path, level: CustomSkillLevel) -> Vec<CustomSkill> {
  let mut found: Vec<CustomSkill> = Vec::new();
  for parent in AGENT_SKILL_PARENTS {
    let Ok(entries) = fs::read_dir(root.join(parent)) else {
      continue;
    };
    let mut dirs: Vec<PathBuf> = entries
      .filter_map(|entry| entry.ok().map(|e| e.path()))
      .filter(|dir| dir.join("SKILL.md").is_file() && is_user_owned_dir(dir))
      .collect();
    dirs.sort();
    for dir in dirs {
      let Some(name) = dir.file_name().map(|n| n.to_string_lossy().into_owned()) else {
        continue;
      };
      // Repository copies are named relative to it, so prompts never carry
      // an absolute path; a global skill keeps its full path to copy from.
      let path = match level {
        CustomSkillLevel::Repo => format!("{parent}/{name}"),
        CustomSkillLevel::Global => dir.to_string_lossy().into_owned(),
      };
      match found.iter_mut().find(|skill| skill.name == name) {
        Some(skill) => skill.paths.push(path),
        None => found.push(CustomSkill {
          name,
          description: skill_description(&dir.join("SKILL.md")),
          level,
          paths: vec![path],
        }),
      }
    }
  }
  found.sort_by(|a, b| a.name.cmp(&b.name));
  found
}

/// User-owned skills in the repository's and the home folder's
/// `.agents/skills` and `.claude/skills`: the repository's first. Folders
/// with the generated-skill marker are Treq's and left out; missing paths
/// give nothing.
pub fn list_custom_skills(repo_path: Option<&str>, home: Option<&Path>) -> Vec<CustomSkill> {
  let mut out = Vec::new();
  if let Some(repo) = repo_path.filter(|p| !p.trim().is_empty()) {
    out.extend(custom_skills_in(Path::new(repo), CustomSkillLevel::Repo));
  }
  if let Some(home) = home {
    out.extend(custom_skills_in(home, CustomSkillLevel::Global));
  }
  out
}

/// The prompt for an agent session that edits a user-owned skill.
pub fn edit_custom_skill_prompt(name: &str, paths: &[String]) -> String {
  let folders = paths
    .iter()
    .map(|p| format!("{p}/"))
    .collect::<Vec<_>>()
    .join(" and ");
  format!(
    "Help me edit the \"{name}\" agent skill at {folders}. Read SKILL.md, ask me what I want to \
     change, then make the edits and keep every copy identical."
  )
}

const MAX_SKILL_ID_LENGTH: usize = 200;

/// The catalog id from a `treq://skill/install?id=...` link, or `None`. The
/// id is only a lookup key: installing reads the catalog entry with that id,
/// never content from the link.
pub fn parse_skill_install_link(url: &str) -> Option<String> {
  let parsed = url::Url::parse(url).ok()?;
  if parsed.scheme() != "treq" || parsed.host_str() != Some("skill") || parsed.path() != "/install"
  {
    return None;
  }
  let id = parsed
    .query_pairs()
    .find(|(key, _)| key == "id")
    .map(|(_, value)| value.into_owned())?;
  let segment_ok = |part: &str| {
    !part.is_empty()
      && part != "."
      && part != ".."
      && part
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
  };
  (id.len() <= MAX_SKILL_ID_LENGTH && id.split('/').all(segment_ok)).then_some(id)
}

/// The prompt for an agent session that edits a customized skill's copy.
pub fn customize_skill_prompt(
  name: &str,
  agents_skill_path: &str,
  claude_skill_path: &str,
) -> String {
  format!(
    "Help me customize the \"{name}\" agent skill. Its user-owned copy is at {agents_skill_path}/ \
     and is mirrored at {claude_skill_path}/. Treq no longer updates these folders from the skill \
     catalog. Read SKILL.md, ask me what I want to change, then make the edits and keep both \
     folders identical."
  )
}

/// Copy every installed library skill into a workspace's agent skill folders.
pub fn materialize_installed_skills(repo_path: &str, workspace_path: &str) -> Result<(), String> {
  let workspace = Path::new(workspace_path);
  if !workspace.exists() {
    return Ok(());
  }
  let installed = list_installed_skills(Some(repo_path))?;
  for record in installed {
    let root = match record.scope {
      SkillInstallScope::Application => match app_skills_root() {
        Ok(root) => root,
        Err(_) => continue,
      },
      SkillInstallScope::Repository => repo_skills_root(repo_path),
    };
    let pack = pack_dir(&root, &record.id)?;
    if !pack.exists() {
      continue;
    }
    let files = read_pack_files(&pack)?;
    let dirname = skill_dirname(&record.name)?;
    install_generated_skill_copy(workspace, ".agents/skills", &dirname, &files)?;
    install_generated_skill_copy(workspace, ".claude/skills", &dirname, &files)?;
  }
  Ok(())
}

pub fn persist_app_index_setting(db: &crate::db::Database) -> Result<(), String> {
  let skills = if let Ok(root) = app_skills_root() {
    load_index(&root)?
  } else {
    Vec::new()
  };
  let json = serde_json::to_string(&skills).map_err(|e| e.to_string())?;
  db.set_setting(APP_SETTING_KEY, &json)
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::AppDataDirGuard;
  use tempfile::TempDir;

  fn sample_files() -> Vec<(String, Vec<u8>)> {
    vec![
      (
        "SKILL.md".to_string(),
        b"---\nname: demo\ndescription: demo skill\n---\n# Demo\n".to_vec(),
      ),
      ("notes.txt".to_string(), b"hello\n".to_vec()),
    ]
  }

  fn sample_entry(checksum: String) -> SkillCatalogEntry {
    SkillCatalogEntry {
      id: "test/demo".to_string(),
      name: "demo".to_string(),
      description: Some("demo skill".to_string()),
      source: "test".to_string(),
      category: None,
      license: Some("MIT".to_string()),
      proprietary: false,
      url: None,
      route: None,
      checksum: Some(checksum),
      files: vec![
        SkillFile {
          path: "SKILL.md".to_string(),
          size: 1,
          binary: false,
          github_url: None,
          raw_url: Some("https://example.test/SKILL.md".to_string()),
        },
        SkillFile {
          path: "notes.txt".to_string(),
          size: 1,
          binary: false,
          github_url: None,
          raw_url: Some("https://example.test/notes.txt".to_string()),
        },
      ],
    }
  }

  #[test]
  fn parse_catalog_keeps_registry_route() {
    let catalog = parse_catalog(
      br#"{"skills":[{"id":"openai/.curated/x","name":"x","source":"openai","route":"/skills/openai/curated/x"}]}"#,
    )
    .expect("parse");
    assert_eq!(
      catalog.skills[0].route.as_deref(),
      Some("/skills/openai/curated/x")
    );
  }

  #[test]
  fn parse_skill_install_link_reads_the_catalog_id() {
    assert_eq!(
      parse_skill_install_link("treq://skill/install?id=anthropic%2Fpdf").as_deref(),
      Some("anthropic/pdf")
    );
    assert_eq!(
      parse_skill_install_link("treq://skill/install?id=openai%2F.curated%2Fx").as_deref(),
      Some("openai/.curated/x")
    );
  }

  #[test]
  fn parse_skill_install_link_rejects_other_routes_and_unsafe_ids() {
    let long = format!("treq://skill/install?id={}", "x".repeat(201));
    for url in [
      "treq://auth/callback?token=abc",
      "treq://skill/uninstall?id=anthropic%2Fpdf",
      "treq://skill/install/extra?id=anthropic%2Fpdf",
      "https://treq.dev/skill/install?id=anthropic%2Fpdf",
      "treq://skill/install",
      "treq://skill/install?id=",
      "treq://skill/install?id=..%2Fetc",
      "treq://skill/install?id=a%2F..%2Fb",
      "treq://skill/install?id=%2Fabs",
      "treq://skill/install?id=a%5Cb",
      "treq://skill/install?id=a%20b",
      "treq://skill/install?id=a%2F",
      long.as_str(),
      "not a url",
    ] {
      assert_eq!(parse_skill_install_link(url), None, "{url}");
    }
  }

  #[test]
  fn customize_prompt_names_both_copies() {
    let prompt = customize_skill_prompt("demo", ".agents/skills/demo", ".claude/skills/demo");
    assert!(prompt.contains("\"demo\""));
    assert!(prompt.contains(".agents/skills/demo/"));
    assert!(prompt.contains(".claude/skills/demo/"));
  }

  fn write_user_skill(root: &Path, parent: &str, name: &str, description: &str) {
    let dir = root.join(parent).join(name);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
      dir.join("SKILL.md"),
      format!("---\nname: {name}\ndescription: {description}\n---\n# {name}\n"),
    )
    .unwrap();
  }

  #[test]
  fn list_custom_skills_finds_user_owned_repo_and_global_skills() {
    let repo = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    write_user_skill(repo.path(), ".agents/skills", "review", "Reviews diffs");
    write_user_skill(repo.path(), ".claude/skills", "review", "Reviews diffs");
    write_user_skill(repo.path(), ".claude/skills", "deploy", "Ships it");
    write_user_skill(home.path(), ".claude/skills", "notes", "Takes notes");
    let found = list_custom_skills(Some(repo.path().to_str().unwrap()), Some(home.path()));
    let summary: Vec<_> = found
      .iter()
      .map(|s| {
        (
          s.name.as_str(),
          s.level,
          s.paths.len(),
          s.description.as_deref(),
        )
      })
      .collect();
    assert_eq!(
      summary,
      [
        ("deploy", CustomSkillLevel::Repo, 1, Some("Ships it")),
        ("review", CustomSkillLevel::Repo, 2, Some("Reviews diffs")),
        ("notes", CustomSkillLevel::Global, 1, Some("Takes notes")),
      ]
    );
    assert_eq!(
      found[1].paths,
      [".agents/skills/review", ".claude/skills/review"]
    );
    assert!(found[2].paths[0].starts_with(home.path().to_str().unwrap()));
  }

  #[test]
  fn list_custom_skills_skips_treq_generated_and_non_skill_dirs() {
    let repo = TempDir::new().unwrap();
    write_user_skill(repo.path(), ".agents/skills", "treq", "bundled");
    fs::write(
      repo
        .path()
        .join(".agents/skills/treq")
        .join(PROJECT_SKILL_MARKER),
      "",
    )
    .unwrap();
    fs::create_dir_all(repo.path().join(".claude/skills/empty")).unwrap();
    fs::write(repo.path().join(".claude/skills/README.md"), "not a skill").unwrap();
    assert!(list_custom_skills(Some(repo.path().to_str().unwrap()), None).is_empty());
  }

  #[test]
  fn list_custom_skills_returns_nothing_for_missing_paths() {
    assert!(list_custom_skills(Some("/no/such/repo"), Some(Path::new("/no/such/home"))).is_empty());
    assert!(list_custom_skills(None, None).is_empty());
  }

  #[test]
  fn edit_custom_skill_prompt_names_its_folders() {
    let prompt = edit_custom_skill_prompt("notes", &[".claude/skills/notes".into()]);
    assert!(prompt.contains("\"notes\""));
    assert!(prompt.contains(".claude/skills/notes/"));
  }

  #[test]
  fn customize_skill_from_files_writes_both_user_owned_copies() {
    let repo = TempDir::new().unwrap();
    let copy = customize_skill_from_files(
      "anthropic/demo",
      "demo",
      &sample_files(),
      repo.path().to_str().unwrap(),
    )
    .expect("customize");
    assert_eq!(copy.agents_skill_path, ".agents/skills/demo");
    for dir in [".agents/skills/demo", ".claude/skills/demo"] {
      assert!(repo.path().join(dir).join("SKILL.md").exists());
      assert!(!repo.path().join(dir).join(PROJECT_SKILL_MARKER).exists());
    }
    let err =
      customize_skill_from_files("x", "demo", &sample_files(), repo.path().to_str().unwrap())
        .expect_err("exists");
    assert!(err.contains("already customized"), "{err}");
  }

  #[test]
  fn customize_global_skill_copies_it_into_the_repo() {
    let repo = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    write_user_skill(home.path(), ".claude/skills", "notes", "Takes notes");
    let source = home.path().join(".claude/skills/notes");
    let copy =
      customize_global_skill("notes", &source, repo.path().to_str().unwrap()).expect("customize");
    assert_eq!(copy.claude_skill_path, ".claude/skills/notes");
    assert_eq!(
      fs::read_to_string(repo.path().join(".agents/skills/notes/SKILL.md")).unwrap(),
      fs::read_to_string(source.join("SKILL.md")).unwrap()
    );
  }

  #[test]
  fn pending_skill_install_is_taken_once() {
    queue_pending_skill_install("anthropic/pdf".into());
    assert_eq!(
      take_pending_skill_install().as_deref(),
      Some("anthropic/pdf")
    );
    assert_eq!(take_pending_skill_install(), None);
  }

  #[test]
  fn skill_checksum_is_stable_for_path_order() {
    let a = sample_files();
    let mut b = sample_files();
    b.reverse();
    assert_eq!(skill_checksum(&a), skill_checksum(&b));
  }

  #[test]
  fn skill_checksum_changes_when_content_changes() {
    let a = sample_files();
    let mut b = sample_files();
    b[1].1 = b"changed\n".to_vec();
    assert_ne!(skill_checksum(&a), skill_checksum(&b));
  }

  #[test]
  fn install_skill_files_rejects_checksum_mismatch() {
    let app = TempDir::new().unwrap();
    let _guard = AppDataDirGuard::set(app.path());
    let files = sample_files();
    let entry = sample_entry("deadbeef".to_string());
    let err = install_skill_files(&entry, &files, SkillInstallScope::Application, None)
      .expect_err("mismatch");
    assert!(err.contains("Checksum mismatch"), "{err}");
  }

  #[test]
  fn install_skill_files_records_computed_checksum_when_catalog_omits_it() {
    let app = TempDir::new().unwrap();
    let _guard = AppDataDirGuard::set(app.path());
    let files = sample_files();
    let mut entry = sample_entry(skill_checksum(&files));
    entry.checksum = None;
    let installed =
      install_skill_files(&entry, &files, SkillInstallScope::Application, None).expect("install");
    assert_eq!(installed.checksum, skill_checksum(&files));
  }

  #[test]
  fn install_skill_files_writes_application_pack() {
    let app = TempDir::new().unwrap();
    let _guard = AppDataDirGuard::set(app.path());
    let files = sample_files();
    let checksum = skill_checksum(&files);
    let entry = sample_entry(checksum.clone());
    let installed =
      install_skill_files(&entry, &files, SkillInstallScope::Application, None).expect("install");
    assert_eq!(installed.scope, SkillInstallScope::Application);
    assert_eq!(installed.checksum, checksum);
    let skill_md = app.path().join("skills/test__demo/SKILL.md");
    assert_eq!(
      fs::read_to_string(skill_md).unwrap(),
      String::from_utf8(files[0].1.clone()).unwrap()
    );
    let listed = list_installed_skills(None).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, "test/demo");
  }

  #[test]
  fn install_skill_files_writes_repository_pack() {
    let app = TempDir::new().unwrap();
    let repo = TempDir::new().unwrap();
    let _guard = AppDataDirGuard::set(app.path());
    let files = sample_files();
    let entry = sample_entry(skill_checksum(&files));
    install_skill_files(
      &entry,
      &files,
      SkillInstallScope::Repository,
      Some(repo.path().to_str().unwrap()),
    )
    .expect("install");
    assert!(repo
      .path()
      .join(".treq/skills/test__demo/SKILL.md")
      .exists());
    assert!(list_installed_skills(None).unwrap().is_empty());
    assert_eq!(
      list_installed_skills(Some(repo.path().to_str().unwrap()))
        .unwrap()
        .len(),
      1
    );
  }

  #[test]
  fn set_skill_install_scope_moves_pack() {
    let app = TempDir::new().unwrap();
    let repo = TempDir::new().unwrap();
    let _guard = AppDataDirGuard::set(app.path());
    let files = sample_files();
    let entry = sample_entry(skill_checksum(&files));
    install_skill_files(&entry, &files, SkillInstallScope::Application, None).expect("install");
    let moved = set_skill_install_scope(
      "test/demo",
      SkillInstallScope::Repository,
      Some(repo.path().to_str().unwrap()),
    )
    .expect("move");
    assert_eq!(moved.scope, SkillInstallScope::Repository);
    assert!(!app.path().join("skills/test__demo/SKILL.md").exists());
    assert!(repo
      .path()
      .join(".treq/skills/test__demo/SKILL.md")
      .exists());
  }

  #[test]
  fn materialize_installed_skills_writes_agent_folders() {
    let app = TempDir::new().unwrap();
    let repo = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let _guard = AppDataDirGuard::set(app.path());
    let files = sample_files();
    let entry = sample_entry(skill_checksum(&files));
    install_skill_files(&entry, &files, SkillInstallScope::Application, None).expect("install");
    materialize_installed_skills(
      repo.path().to_str().unwrap(),
      workspace.path().to_str().unwrap(),
    )
    .expect("materialize");
    assert!(workspace
      .path()
      .join(".agents/skills/demo/SKILL.md")
      .exists());
    assert!(workspace
      .path()
      .join(".claude/skills/demo/SKILL.md")
      .exists());
    assert!(workspace
      .path()
      .join(".agents/skills/demo/.treq-generated")
      .exists());
    assert!(workspace
      .path()
      .join(".claude/skills/demo/.gitignore")
      .exists());
  }

  #[test]
  fn materialize_installed_skills_skips_user_owned_skill_dir() {
    let app = TempDir::new().unwrap();
    let repo = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let _guard = AppDataDirGuard::set(app.path());
    let files = sample_files();
    let entry = sample_entry(skill_checksum(&files));
    install_skill_files(&entry, &files, SkillInstallScope::Application, None).expect("install");
    let user_dir = workspace.path().join(".agents/skills/demo");
    fs::create_dir_all(&user_dir).unwrap();
    fs::write(user_dir.join("SKILL.md"), "user skill\n").unwrap();
    materialize_installed_skills(
      repo.path().to_str().unwrap(),
      workspace.path().to_str().unwrap(),
    )
    .expect("materialize");
    assert_eq!(
      fs::read_to_string(user_dir.join("SKILL.md")).unwrap(),
      "user skill\n"
    );
  }

  fn install_sample() -> SkillCatalogEntry {
    let files = sample_files();
    let entry = sample_entry(skill_checksum(&files));
    install_skill_files(&entry, &files, SkillInstallScope::Application, None).expect("install");
    entry
  }

  #[test]
  fn customize_skill_writes_user_owned_copies() {
    let app = TempDir::new().unwrap();
    let repo = TempDir::new().unwrap();
    let _guard = AppDataDirGuard::set(app.path());
    install_sample();
    let customized =
      customize_skill("test/demo", repo.path().to_str().unwrap()).expect("customize");
    assert_eq!(customized.agents_skill_path, ".agents/skills/demo");
    assert_eq!(customized.claude_skill_path, ".claude/skills/demo");
    for dir in [".agents/skills/demo", ".claude/skills/demo"] {
      let dir = repo.path().join(dir);
      assert!(dir.join("SKILL.md").exists());
      assert!(dir.join("notes.txt").exists());
      assert!(!dir.join(PROJECT_SKILL_MARKER).exists());
    }
  }

  #[test]
  fn customize_skill_replaces_generated_copy() {
    let app = TempDir::new().unwrap();
    let repo = TempDir::new().unwrap();
    let _guard = AppDataDirGuard::set(app.path());
    install_sample();
    let repo_path = repo.path().to_str().unwrap();
    materialize_installed_skills(repo_path, repo_path).expect("materialize");
    let generated = repo.path().join(".agents/skills/demo");
    assert!(generated.join(PROJECT_SKILL_MARKER).exists());
    customize_skill("test/demo", repo_path).expect("customize");
    assert!(!generated.join(PROJECT_SKILL_MARKER).exists());
    assert!(generated.join("SKILL.md").exists());
  }

  #[test]
  fn customize_skill_keeps_existing_user_copy() {
    let app = TempDir::new().unwrap();
    let repo = TempDir::new().unwrap();
    let _guard = AppDataDirGuard::set(app.path());
    install_sample();
    let user_dir = repo.path().join(".claude/skills/demo");
    fs::create_dir_all(&user_dir).unwrap();
    fs::write(user_dir.join("SKILL.md"), "mine\n").unwrap();
    let err = customize_skill("test/demo", repo.path().to_str().unwrap()).expect_err("exists");
    assert!(err.contains("already customized"), "{err}");
    assert_eq!(
      fs::read_to_string(user_dir.join("SKILL.md")).unwrap(),
      "mine\n"
    );
    assert!(!repo.path().join(".agents/skills/demo").exists());
  }

  #[test]
  fn customize_skill_rejects_skill_that_is_not_installed() {
    let app = TempDir::new().unwrap();
    let repo = TempDir::new().unwrap();
    let _guard = AppDataDirGuard::set(app.path());
    let err = customize_skill("test/demo", repo.path().to_str().unwrap()).expect_err("missing");
    assert!(err.contains("not installed"), "{err}");
  }

  #[test]
  fn customized_skill_survives_catalog_update() {
    let app = TempDir::new().unwrap();
    let repo = TempDir::new().unwrap();
    let _guard = AppDataDirGuard::set(app.path());
    install_sample();
    let repo_path = repo.path().to_str().unwrap();
    customize_skill("test/demo", repo_path).expect("customize");
    let skill_md = repo.path().join(".agents/skills/demo/SKILL.md");
    fs::write(&skill_md, "edited\n").unwrap();

    let mut updated = sample_files();
    updated[0].1 = b"---\nname: demo\n---\n# Demo v2\n".to_vec();
    let mut entry = sample_entry(skill_checksum(&updated));
    entry.checksum = Some(skill_checksum(&updated));
    install_skill_files(&entry, &updated, SkillInstallScope::Application, None).expect("update");
    materialize_installed_skills(repo_path, repo_path).expect("materialize");

    assert_eq!(fs::read_to_string(&skill_md).unwrap(), "edited\n");
  }

  #[test]
  fn merge_catalog_marks_customized_skills() {
    let app = TempDir::new().unwrap();
    let repo = TempDir::new().unwrap();
    let _guard = AppDataDirGuard::set(app.path());
    let entry = install_sample();
    let repo_path = repo.path().to_str().unwrap();
    let catalog = || SkillCatalog {
      generated_at: None,
      sources: vec![],
      skills: vec![entry.clone()],
    };
    materialize_installed_skills(repo_path, repo_path).expect("materialize");
    let view = merge_catalog_with_installed(catalog(), Some(repo_path)).unwrap();
    assert!(!view.skills[0].customized);
    customize_skill("test/demo", repo_path).expect("customize");
    let view = merge_catalog_with_installed(catalog(), Some(repo_path)).unwrap();
    assert!(view.skills[0].customized);
  }

  #[test]
  fn write_skill_pack_keeps_old_pack_when_new_files_are_invalid() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("pack");
    write_skill_pack(&dest, &sample_files()).unwrap();
    let bad = vec![
      ("SKILL.md".to_string(), b"new".to_vec()),
      ("../escape".to_string(), b"x".to_vec()),
    ];
    assert!(write_skill_pack(&dest, &bad).is_err());
    assert_eq!(
      fs::read(dest.join("SKILL.md")).unwrap(),
      sample_files()[0].1
    );
    assert!(dest.join("notes.txt").exists());
    let leftovers: Vec<_> = fs::read_dir(dir.path()).unwrap().collect();
    assert_eq!(leftovers.len(), 1, "staging dir was not cleaned up");
  }

  #[cfg(unix)]
  #[test]
  fn copy_dir_recursive_does_not_follow_symlinks_outside_source() {
    let dir = tempfile::tempdir().unwrap();
    let outside = dir.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("secret.txt"), "secret").unwrap();
    fs::write(dir.path().join("outside-file.txt"), "secret").unwrap();
    let src = dir.path().join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("SKILL.md"), "ok").unwrap();
    std::os::unix::fs::symlink(&outside, src.join("dir-link")).unwrap();
    std::os::unix::fs::symlink(dir.path().join("outside-file.txt"), src.join("file-link")).unwrap();
    let dest = dir.path().join("dest");
    copy_dir_recursive(&src, &dest).unwrap();
    assert!(dest.join("SKILL.md").exists());
    assert!(!dest.join("dir-link").exists());
    assert!(!dest.join("file-link").exists());
  }

  #[test]
  fn write_skill_pack_rejects_path_escape() {
    let dest = TempDir::new().unwrap();
    let err = write_skill_pack(
      dest.path(),
      &[("../escape.md".to_string(), b"nope".to_vec())],
    )
    .expect_err("escape");
    assert!(err.contains("Unsafe"), "{err}");
  }

  #[test]
  fn merge_catalog_marks_installed_skills() {
    let app = TempDir::new().unwrap();
    let _guard = AppDataDirGuard::set(app.path());
    let files = sample_files();
    let entry = sample_entry(skill_checksum(&files));
    install_skill_files(&entry, &files, SkillInstallScope::Application, None).expect("install");
    let catalog = SkillCatalog {
      generated_at: None,
      sources: vec![],
      skills: vec![entry],
    };
    let view = merge_catalog_with_installed(catalog, None).unwrap();
    assert_eq!(view.skills[0].installed.as_ref().unwrap().id, "test/demo");
  }
}
