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
      SkillCatalogSkillView { skill, installed }
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
  let _ = fs::remove_dir_all(&staging);
  let _ = fs::remove_dir_all(&backup);
  if let Err(err) = fs::create_dir_all(&staging)
    .map_err(|e| format!("Failed to create skill pack dir: {e}"))
    .and_then(|()| build(&staging))
  {
    let _ = fs::remove_dir_all(&staging);
    return Err(err);
  }
  let had_old = dest.exists();
  if had_old {
    if let Err(e) = fs::rename(dest, &backup) {
      let _ = fs::remove_dir_all(&staging);
      return Err(format!("Failed to replace skill pack: {e}"));
    }
  }
  if let Err(e) = fs::rename(&staging, dest) {
    if had_old {
      let _ = fs::rename(&backup, dest);
    }
    let _ = fs::remove_dir_all(&staging);
    return Err(format!("Failed to replace skill pack: {e}"));
  }
  if had_old {
    let _ = fs::remove_dir_all(&backup);
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
    let _ = uninstall_from_root(&other, &entry.id);
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
