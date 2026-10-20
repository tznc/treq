//! Which editor applications ("Open in Cursor", VS Code, Zed) are
//! installed, for the "Open in…" menus. Detected once, then kept in memory
//! (`binary_paths`) and in the app settings as `editor_app_<key>`.

use std::collections::HashMap;

use crate::{binary_paths, db::Database};

/// The editors the menus offer: application name and key.
pub const EDITORS: [(&str, &str); 3] = [
  ("Cursor", "cursor"),
  ("Visual Studio Code", "vscode"),
  ("Zed", "zed"),
];

/// The installed editors by key, detected now unless already known.
pub fn detect(db: &Database) -> HashMap<String, bool> {
  if let Some(cached) = binary_paths::get_editor_apps_cache() {
    return cached;
  }
  let apps = detect_with(db, binary_paths::detect_editor_app);
  binary_paths::init_editor_apps_cache(apps.clone());
  apps
}

/// Detects each editor with `installed` and records the answers in `db`.
pub fn detect_with(db: &Database, installed: impl Fn(&str) -> bool) -> HashMap<String, bool> {
  EDITORS
    .iter()
    .map(|(name, key)| {
      let found = installed(name);
      log::debug!(
        "Editor app {name}: {}",
        if found { "found" } else { "not found" }
      );
      let value = if found { "true" } else { "false" };
      if let Err(err) = db.set_setting(&format!("editor_app_{key}"), value) {
        log::warn!("Failed to cache {key} in database: {err}");
      }
      (key.to_string(), found)
    })
    .collect()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn records_each_editor_found() {
    let dir = tempfile::TempDir::new().unwrap();
    let db = Database::new(dir.path().join("app.db")).unwrap();
    db.init().unwrap();
    let apps = detect_with(&db, |name| name == "Zed");
    assert_eq!(apps["zed"], true);
    assert_eq!(apps["cursor"], false);
    assert_eq!(
      db.get_setting("editor_app_zed").unwrap().as_deref(),
      Some("true")
    );
    assert_eq!(
      db.get_setting("editor_app_vscode").unwrap().as_deref(),
      Some("false")
    );
  }
}
