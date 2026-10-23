use crate::core::skills::{
  catalog_url, install_skill_files, merge_catalog_with_installed, parse_catalog,
  read_bytes_from_locator, InstalledSkill, SkillCatalogEntry, SkillCatalogView, SkillInstallScope,
};

async fn fetch_bytes(locator: &str) -> Result<Vec<u8>, String> {
  if !locator.contains("://") || locator.starts_with("file://") {
    return read_bytes_from_locator(locator);
  }
  let client = reqwest::Client::builder()
    .connect_timeout(std::time::Duration::from_secs(10))
    .timeout(std::time::Duration::from_secs(60))
    .build()
    .map_err(|e| format!("Failed to fetch {locator}: {e}"))?;
  let response = client
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
