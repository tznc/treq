use crate::core::skills::{
  catalog_url, install_skill_files, merge_catalog_with_installed, parse_catalog,
  persist_app_index_setting, read_bytes_from_locator, InstalledSkill, SkillCatalogEntry,
  SkillCatalogView, SkillInstallScope,
};
use crate::lock_ext::LockExt;
use crate::AppState;
use tauri::State;

async fn fetch_bytes(locator: &str) -> Result<Vec<u8>, String> {
  if !locator.contains("://") || locator.starts_with("file://") {
    return read_bytes_from_locator(locator);
  }
  let response = reqwest::Client::new()
    .get(locator)
    .header("user-agent", "treq-skills")
    .send()
    .await
    .map_err(|e| format!("Failed to fetch {locator}: {e}"))?;
  if !response.status().is_success() {
    return Err(format!(
      "Failed to fetch {locator}: HTTP {}",
      response.status()
    ));
  }
  response
    .bytes()
    .await
    .map(|b| b.to_vec())
    .map_err(|e| format!("Failed to read {locator}: {e}"))
}

async fn load_catalog(
  catalog_url_override: Option<&str>,
) -> Result<crate::core::skills::SkillCatalog, String> {
  let url = catalog_url(catalog_url_override);
  let bytes = fetch_bytes(&url).await?;
  parse_catalog(&bytes)
}

async fn download_skill_files(entry: &SkillCatalogEntry) -> Result<Vec<(String, Vec<u8>)>, String> {
  let mut files = Vec::new();
  for file in &entry.files {
    let url = file
      .raw_url
      .as_deref()
      .map(str::trim)
      .filter(|s| !s.is_empty())
      .ok_or_else(|| format!("Skill file '{}' has no download URL", file.path))?;
    let bytes = fetch_bytes(url).await?;
    files.push((file.path.clone(), bytes));
  }
  Ok(files)
}

/// The registry's skills, each with its install state for `repo_path`.
pub async fn catalog_view(
  repo_path: Option<&str>,
  catalog_url: Option<&str>,
) -> Result<SkillCatalogView, String> {
  let catalog = load_catalog(catalog_url).await?;
  merge_catalog_with_installed(catalog, repo_path)
}

/// Downloads `skill_id` from the registry and installs it at `scope`.
/// Callers then record the app index (`persist_app_index_setting`).
pub async fn download_and_install(
  skill_id: &str,
  scope: SkillInstallScope,
  repo_path: Option<&str>,
  catalog_url: Option<&str>,
) -> Result<InstalledSkill, String> {
  let catalog = load_catalog(catalog_url).await?;
  let entry = catalog
    .skills
    .into_iter()
    .find(|skill| skill.id == skill_id)
    .ok_or_else(|| format!("Skill '{skill_id}' was not found in the catalog"))?;
  let files = download_skill_files(&entry).await?;
  install_skill_files(&entry, &files, scope, repo_path)
}

#[tauri::command]
pub async fn list_skill_catalog(
  state: State<'_, AppState>,
  repo_path: Option<String>,
  catalog_url: Option<String>,
) -> Result<SkillCatalogView, String> {
  crate::commands::feature_preview::require(
    &state,
    crate::core::feature_preview::PreviewFeature::SkillsInstallation,
  )?;
  catalog_view(repo_path.as_deref(), catalog_url.as_deref()).await
}

#[tauri::command]
pub fn list_installed_skills(
  state: State<'_, AppState>,
  repo_path: Option<String>,
) -> Result<Vec<InstalledSkill>, String> {
  crate::commands::feature_preview::require(
    &state,
    crate::core::feature_preview::PreviewFeature::SkillsInstallation,
  )?;
  crate::core::skills::list_installed_skills(repo_path.as_deref())
}

#[tauri::command]
pub async fn install_skill(
  state: State<'_, AppState>,
  skill_id: String,
  scope: SkillInstallScope,
  repo_path: Option<String>,
  catalog_url: Option<String>,
) -> Result<InstalledSkill, String> {
  crate::commands::feature_preview::require(
    &state,
    crate::core::feature_preview::PreviewFeature::SkillsInstallation,
  )?;
  let installed = download_and_install(
    &skill_id,
    scope,
    repo_path.as_deref(),
    catalog_url.as_deref(),
  )
  .await?;
  let db = state.db.lock_or_recover();
  persist_app_index_setting(&db)?;
  Ok(installed)
}

#[tauri::command]
pub fn uninstall_skill(
  state: State<'_, AppState>,
  skill_id: String,
  repo_path: Option<String>,
) -> Result<(), String> {
  crate::commands::feature_preview::require(
    &state,
    crate::core::feature_preview::PreviewFeature::SkillsInstallation,
  )?;
  crate::core::skills::uninstall_skill(&skill_id, repo_path.as_deref())?;
  let db = state.db.lock_or_recover();
  persist_app_index_setting(&db)?;
  Ok(())
}

#[tauri::command]
pub fn set_skill_install_scope(
  state: State<'_, AppState>,
  skill_id: String,
  scope: SkillInstallScope,
  repo_path: Option<String>,
) -> Result<InstalledSkill, String> {
  crate::commands::feature_preview::require(
    &state,
    crate::core::feature_preview::PreviewFeature::SkillsInstallation,
  )?;
  let installed =
    crate::core::skills::set_skill_install_scope(&skill_id, scope, repo_path.as_deref())?;
  let db = state.db.lock_or_recover();
  persist_app_index_setting(&db)?;
  Ok(installed)
}
