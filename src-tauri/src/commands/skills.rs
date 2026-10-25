use crate::core::skills::{
  catalog_url, install_skill_files, merge_catalog_with_installed, parse_catalog,
  read_bytes_from_locator, InstalledSkill, SkillCatalog, SkillCatalogEntry, SkillCatalogView,
  SkillInstallScope,
};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// A catalog read this recently is used as it is, so reopening the tab or
/// reading again after an install asks the network for nothing.
const CATALOG_FRESH: Duration = Duration::from_secs(30);
/// An install looks its skill up in a catalog read this recently.
const CATALOG_INSTALL: Duration = Duration::from_secs(600);

fn is_remote(locator: &str) -> bool {
  locator.contains("://") && !locator.starts_with("file://")
}

/// The one client every download shares, so a request after the first
/// reuses its TLS configuration and connection.
fn http_client() -> Result<&'static reqwest::Client, String> {
  static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
  if let Some(client) = CLIENT.get() {
    return Ok(client);
  }
  let client = reqwest::Client::builder()
    .connect_timeout(Duration::from_secs(10))
    .timeout(Duration::from_secs(60))
    .build()
    .map_err(|e| format!("Failed to create the HTTP client: {e}"))?;
  Ok(CLIENT.get_or_init(|| client))
}

async fn fetch_bytes(locator: &str) -> Result<Vec<u8>, String> {
  if !is_remote(locator) {
    return read_bytes_from_locator(locator);
  }
  let response = http_client()?
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

/// The registries read from the network, by URL, and when.
static CATALOGS: Mutex<Option<HashMap<String, (Instant, SkillCatalog)>>> = Mutex::new(None);

/// The catalog at `url` as read from the network within `max_age`.
fn cached_catalog(url: &str, max_age: Duration) -> Option<SkillCatalog> {
  let catalogs = CATALOGS.lock().unwrap_or_else(|p| p.into_inner());
  let (read_at, catalog) = catalogs.as_ref()?.get(url)?;
  (read_at.elapsed() <= max_age).then(|| catalog.clone())
}

/// The catalog as of its last read, however long ago, without the network.
fn last_catalog(url: &str) -> Option<SkillCatalog> {
  cached_catalog(url, Duration::MAX)
}

async fn load_catalog(
  catalog_url_override: Option<&str>,
  max_age: Duration,
) -> Result<SkillCatalog, String> {
  let url = catalog_url(catalog_url_override);
  if !is_remote(&url) {
    return parse_catalog(&fetch_bytes(&url).await?);
  }
  if let Some(catalog) = cached_catalog(&url, max_age) {
    return Ok(catalog);
  }
  let catalog = parse_catalog(&fetch_bytes(&url).await?)?;
  CATALOGS
    .lock()
    .unwrap_or_else(|p| p.into_inner())
    .get_or_insert_with(HashMap::new)
    .insert(url, (Instant::now(), catalog.clone()));
  Ok(catalog)
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
  let catalog = load_catalog(catalog_url, CATALOG_FRESH).await?;
  merge_catalog_with_installed(catalog, repo_path)
}

/// [`catalog_view`] from the last registry read, however old, without the
/// network: what a tab shows while it reads the registry again.
pub fn cached_catalog_view(
  repo_path: Option<&str>,
  catalog_url_override: Option<&str>,
) -> Option<SkillCatalogView> {
  let catalog = last_catalog(&catalog_url(catalog_url_override))?;
  merge_catalog_with_installed(catalog, repo_path).ok()
}

/// Downloads `skill_id` from the registry and installs it at `scope`.
/// Callers then record the app index (`persist_app_index_setting`).
pub async fn download_and_install(
  skill_id: &str,
  scope: SkillInstallScope,
  repo_path: Option<&str>,
  catalog_url: Option<&str>,
) -> Result<InstalledSkill, String> {
  // The catalog the list was drawn from, not a second read of it; one read
  // again only when it does not list the skill.
  let mut catalog = load_catalog(catalog_url, CATALOG_INSTALL).await?;
  if !catalog.skills.iter().any(|skill| skill.id == skill_id) {
    catalog = load_catalog(catalog_url, Duration::ZERO).await?;
  }
  let entry = catalog
    .skills
    .into_iter()
    .find(|skill| skill.id == skill_id)
    .ok_or_else(|| format!("Skill '{skill_id}' was not found in the catalog"))?;
  let files = download_skill_files(&entry).await?;
  install_skill_files(&entry, &files, scope, repo_path)
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::io::{Read, Write};
  use std::net::TcpListener;
  use std::sync::atomic::{AtomicUsize, Ordering};
  use std::sync::Arc;

  /// Serves `{"skills": []}` to every request, counting them.
  fn serve() -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/catalog.json", listener.local_addr().unwrap());
    let requests = Arc::new(AtomicUsize::new(0));
    let counted = requests.clone();
    std::thread::spawn(move || {
      for stream in listener.incoming() {
        let Ok(mut stream) = stream else { return };
        let mut buffer = [0u8; 2048];
        let _ = stream.read(&mut buffer);
        counted.fetch_add(1, Ordering::SeqCst);
        let body = r#"{"skills":[]}"#;
        let _ = write!(
          stream,
          "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
          body.len()
        );
      }
    });
    (url, requests)
  }

  #[test]
  fn a_catalog_read_twice_in_a_row_asks_the_network_once() {
    let (url, requests) = serve();
    let read = |max_age| tauri::async_runtime::block_on(load_catalog(Some(&url), max_age));
    read(CATALOG_FRESH).unwrap();
    read(CATALOG_FRESH).unwrap();
    read(CATALOG_INSTALL).unwrap();
    assert_eq!(requests.load(Ordering::SeqCst), 1);
    // A read that wants it fresher than it is goes to the network.
    read(Duration::ZERO).unwrap();
    assert_eq!(requests.load(Ordering::SeqCst), 2);
  }

  #[test]
  fn the_last_catalog_is_there_to_show_while_the_next_is_read() {
    let (url, _) = serve();
    assert!(last_catalog(&url).is_none());
    tauri::async_runtime::block_on(load_catalog(Some(&url), CATALOG_FRESH)).unwrap();
    assert!(last_catalog(&url).is_some());
  }

  #[test]
  fn installing_a_skill_missing_from_the_cached_catalog_reads_it_once_more() {
    let (url, requests) = serve();
    tauri::async_runtime::block_on(load_catalog(Some(&url), CATALOG_FRESH)).unwrap();
    let result = tauri::async_runtime::block_on(download_and_install(
      "missing",
      SkillInstallScope::Application,
      None,
      Some(&url),
    ));
    assert!(result.unwrap_err().contains("not found"));
    assert_eq!(requests.load(Ordering::SeqCst), 2);
  }
}
