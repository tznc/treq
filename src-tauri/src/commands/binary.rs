use crate::binary_paths;
use crate::db::Database;
use std::collections::HashMap;

const BINARIES: [&str; 6] = ["git", "jj", "claude", "codex", "cursor-agent", "copilot"];
/// How long a lookup that found nothing is trusted before it is repeated.
const NOT_FOUND_TTL_SECS: u64 = 24 * 60 * 60;

fn missing_key(binary: &str) -> String {
  format!("binary_missing_{binary}")
}

fn now_secs() -> u64 {
  std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map_or(0, |d| d.as_secs())
}

/// Whether `binary` was looked for within [`NOT_FOUND_TTL_SECS`] and not found.
fn recently_missing(db: &Database, binary: &str, now: u64) -> bool {
  db.get_setting(&missing_key(binary))
    .ok()
    .flatten()
    .and_then(|at| at.parse::<u64>().ok())
    .is_some_and(|at| now.saturating_sub(at) < NOT_FOUND_TTL_SECS)
}

/// Load cached binary paths from the database on startup, without running
/// anything. Also returns the binaries that are neither cached nor known to
/// be missing: [`detect_binaries`] looks for them.
pub fn load_cached_binary_paths(db: &Database) -> (HashMap<String, String>, Vec<&'static str>) {
  let now = now_secs();
  let mut paths = HashMap::new();
  let mut undetected = Vec::new();

  for binary in BINARIES {
    if let Ok(Some(path)) = db.get_setting(&format!("binary_path_{binary}")) {
      log::info!("Loaded cached {} path: {}", binary, path);
      paths.insert(binary.to_string(), path);
    } else if !recently_missing(db, binary, now) {
      undetected.push(binary);
    }
  }

  (paths, undetected)
}

/// Looks for `binaries` and caches what was found, or that nothing was, for
/// the next launch. Each lookup may wait on the login shell's PATH, so this
/// runs off the startup path; the database lock is held only to write.
pub fn detect_binaries(db: &std::sync::Mutex<Database>, binaries: &[&str]) {
  use crate::lock_ext::LockExt;
  for binary in binaries {
    let detected = binary_paths::detect_binary(binary);
    let db = db.lock_or_recover();
    let result = match &detected {
      Some(path) => {
        log::info!("Detected {} at: {}", binary, path);
        db.set_setting(&format!("binary_path_{binary}"), path)
      }
      None => db.set_setting(&missing_key(binary), &now_secs().to_string()),
    };
    if let Err(e) = result {
      log::warn!("Failed to cache {} path: {}", binary, e);
    }
  }
}

/// Return the directory containing the running treq binary
pub fn get_treq_bin_dir() -> Result<String, String> {
  binary_paths::get_exe_dir().ok_or_else(|| "Failed to determine treq binary directory".to_string())
}

/// Load cached editor apps from the database on startup, without running
/// anything. `None` when an editor was never detected: the window's
/// `detect_editors` detects them in the background.
pub fn load_cached_editor_apps(db: &Database) -> Option<HashMap<String, bool>> {
  ["cursor", "vscode", "zed"]
    .into_iter()
    .map(|key| {
      let value = db
        .get_setting(&format!("editor_app_{key}"))
        .ok()
        .flatten()?;
      Some((key.to_string(), value == "true"))
    })
    .collect()
}
