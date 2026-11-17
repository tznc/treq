use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct RepoConfig {
  pub branch_name_pattern: Option<String>,
  pub default_model: Option<String>,
  pub default_agent: Option<String>,
  pub target_branch: Option<String>,
  pub included_copy_files: Option<Vec<String>>,
  /// Shell command run once in a new workspace right after it's created.
  /// Checks (`.treq/workflows`) are blocked for that workspace until it finishes.
  pub setup_script: Option<String>,
  /// Custom prompt for the AI review agent, overriding the built-in default.
  pub review_prompt: Option<String>,
  /// Agent to launch for reviews, falling back to `default_agent` when unset.
  pub review_agent: Option<String>,
  /// When reviews fire automatically: `"off"`, `"on-commit"`, `"on-rebase"`
  /// or `"on-pull"`. Read by `auto_review` after each jj operation.
  pub auto_review_trigger: Option<String>,
}

pub fn config_path(repo_path: &str) -> PathBuf {
  Path::new(repo_path).join(".treq").join("config.yaml")
}

/// Parse `.treq/config.yaml`. Returns a default (all-`None`) config if the file
/// does not exist.
pub fn parse_config(repo_path: &str) -> Result<RepoConfig, String> {
  let path = config_path(repo_path);
  if !path.exists() {
    return Ok(RepoConfig::default());
  }
  let content =
    std::fs::read_to_string(&path).map_err(|e| format!("Failed to read .treq/config.yaml: {e}"))?;
  serde_yaml::from_str::<RepoConfig>(&content)
    .map_err(|e| format!("Failed to parse .treq/config.yaml: {e}"))
}

/// Write every field the YAML config sets into the same per-repo settings store
/// that `get_repo_setting`/`set_repo_setting` already use, so every existing
/// consumer of those settings picks up the config file's values without needing
/// to know about `.treq/config.yaml` at all. Fields the config file leaves unset
/// are left untouched, preserving whatever the user set directly.
pub fn sync_to_settings(
  db: &crate::db::Database,
  repo_path: &str,
  config: &RepoConfig,
) -> Result<(), String> {
  if let Some(ref value) = config.branch_name_pattern {
    db.set_repo_setting(repo_path, "branch_name_pattern", value)
      .map_err(|e| format!("Failed to sync branch_name_pattern: {e}"))?;
  }
  if let Some(ref value) = config.default_model {
    db.set_repo_setting(repo_path, "default_model", value)
      .map_err(|e| format!("Failed to sync default_model: {e}"))?;
  }
  if let Some(ref value) = config.default_agent {
    db.set_repo_setting(repo_path, "default_agent", value)
      .map_err(|e| format!("Failed to sync default_agent: {e}"))?;
  }
  if let Some(ref value) = config.review_prompt {
    db.set_repo_setting(repo_path, "review_prompt", value)
      .map_err(|e| format!("Failed to sync review_prompt: {e}"))?;
  }
  if let Some(ref value) = config.review_agent {
    db.set_repo_setting(repo_path, "review_agent", value)
      .map_err(|e| format!("Failed to sync review_agent: {e}"))?;
  }
  if let Some(ref value) = config.auto_review_trigger {
    db.set_repo_setting(repo_path, "auto_review_trigger", value)
      .map_err(|e| format!("Failed to sync auto_review_trigger: {e}"))?;
  }
  if let Some(ref files) = config.included_copy_files {
    db.set_repo_setting(repo_path, "included_copy_files", &files.join("\n"))
      .map_err(|e| format!("Failed to sync included_copy_files: {e}"))?;
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::fs;
  use tempfile::TempDir;

  #[test]
  fn test_config_path_is_dotreq_config_yaml() {
    let path = config_path("/home/user/myrepo");
    assert_eq!(path, Path::new("/home/user/myrepo/.treq/config.yaml"));
  }

  #[test]
  fn test_parse_config_returns_default_when_no_file() {
    let temp_dir = TempDir::new().unwrap();
    let repo_path = temp_dir.path().to_str().unwrap();

    let config = parse_config(repo_path).expect("should succeed with no file");
    assert_eq!(config, RepoConfig::default());
  }

  #[test]
  fn test_parse_config_reads_branch_name_pattern() {
    let temp_dir = TempDir::new().unwrap();
    let repo_path = temp_dir.path().to_str().unwrap();

    let treq_dir = temp_dir.path().join(".treq");
    fs::create_dir_all(&treq_dir).unwrap();
    fs::write(
      treq_dir.join("config.yaml"),
      "branch_name_pattern: \"feat/{name}\"\n",
    )
    .unwrap();

    let config = parse_config(repo_path).expect("should parse");
    assert_eq!(config.branch_name_pattern.as_deref(), Some("feat/{name}"));
  }

  #[test]
  fn test_parse_config_reads_all_fields() {
    let temp_dir = TempDir::new().unwrap();
    let repo_path = temp_dir.path().to_str().unwrap();

    let treq_dir = temp_dir.path().join(".treq");
    fs::create_dir_all(&treq_dir).unwrap();
    fs::write(
      treq_dir.join("config.yaml"),
      "branch_name_pattern: \"treq/{name}\"\n\
             default_model: \"claude-opus-4\"\n\
             default_agent: \"code\"\n\
             target_branch: \"main\"\n\
             included_copy_files:\n\
               - \"src/main.rs\"\n\
               - \"Cargo.toml\"\n\
             setup_script: \"npm install\"\n",
    )
    .unwrap();

    let config = parse_config(repo_path).expect("should parse all fields");
    assert_eq!(config.branch_name_pattern.as_deref(), Some("treq/{name}"));
    assert_eq!(config.default_model.as_deref(), Some("claude-opus-4"));
    assert_eq!(config.default_agent.as_deref(), Some("code"));
    assert_eq!(config.target_branch.as_deref(), Some("main"));
    assert_eq!(
      config.included_copy_files,
      Some(vec!["src/main.rs".to_string(), "Cargo.toml".to_string()])
    );
    assert_eq!(config.setup_script.as_deref(), Some("npm install"));
  }

  #[test]
  fn test_parse_config_reads_setup_script() {
    let temp_dir = TempDir::new().unwrap();
    let repo_path = temp_dir.path().to_str().unwrap();

    let treq_dir = temp_dir.path().join(".treq");
    fs::create_dir_all(&treq_dir).unwrap();
    fs::write(
      treq_dir.join("config.yaml"),
      "setup_script: \"npm ci && npm run build\"\n",
    )
    .unwrap();

    let config = parse_config(repo_path).expect("should parse");
    assert_eq!(
      config.setup_script.as_deref(),
      Some("npm ci && npm run build")
    );
  }

  #[test]
  fn test_parse_config_partial_config() {
    let temp_dir = TempDir::new().unwrap();
    let repo_path = temp_dir.path().to_str().unwrap();

    let treq_dir = temp_dir.path().join(".treq");
    fs::create_dir_all(&treq_dir).unwrap();
    fs::write(treq_dir.join("config.yaml"), "target_branch: \"develop\"\n").unwrap();

    let config = parse_config(repo_path).expect("should handle partial config");
    assert_eq!(config.target_branch.as_deref(), Some("develop"));
    assert!(config.branch_name_pattern.is_none());
    assert!(config.default_model.is_none());
    assert!(config.default_agent.is_none());
    assert!(config.included_copy_files.is_none());
  }

  fn temp_db() -> (TempDir, crate::db::Database) {
    let temp_dir = TempDir::new().unwrap();
    let db = crate::db::Database::new(temp_dir.path().join("test.db")).unwrap();
    db.init().unwrap();
    (temp_dir, db)
  }

  #[test]
  fn sync_to_settings_writes_yaml_fields_into_repo_settings() {
    let (_dir, db) = temp_db();
    let config = RepoConfig {
      branch_name_pattern: Some("treq/{name}".to_string()),
      default_model: Some("opus".to_string()),
      default_agent: Some("claude".to_string()),
      target_branch: Some("main".to_string()),
      included_copy_files: Some(vec!["a.env".to_string(), "b.env".to_string()]),
      setup_script: None,
      review_prompt: None,
      review_agent: None,
      auto_review_trigger: None,
    };

    sync_to_settings(&db, "/repo/a", &config).expect("sync should succeed");

    assert_eq!(
      db.get_repo_setting("/repo/a", "branch_name_pattern")
        .unwrap(),
      Some("treq/{name}".to_string())
    );
    assert_eq!(
      db.get_repo_setting("/repo/a", "default_model").unwrap(),
      Some("opus".to_string())
    );
    assert_eq!(
      db.get_repo_setting("/repo/a", "default_agent").unwrap(),
      Some("claude".to_string())
    );
    assert_eq!(
      db.get_repo_setting("/repo/a", "included_copy_files")
        .unwrap(),
      Some("a.env\nb.env".to_string())
    );
  }

  #[test]
  fn sync_to_settings_leaves_unset_fields_untouched() {
    let (_dir, db) = temp_db();
    db.set_repo_setting("/repo/a", "default_agent", "codex")
      .unwrap();

    let config = RepoConfig {
      default_model: Some("opus".to_string()),
      ..Default::default()
    };
    sync_to_settings(&db, "/repo/a", &config).expect("sync should succeed");

    // default_agent wasn't in the YAML, so the user's own value survives.
    assert_eq!(
      db.get_repo_setting("/repo/a", "default_agent").unwrap(),
      Some("codex".to_string())
    );
    assert_eq!(
      db.get_repo_setting("/repo/a", "branch_name_pattern")
        .unwrap(),
      None
    );
  }

  #[test]
  fn sync_to_settings_overwrites_previously_set_value() {
    let (_dir, db) = temp_db();
    db.set_repo_setting("/repo/a", "default_model", "sonnet")
      .unwrap();

    let config = RepoConfig {
      default_model: Some("opus".to_string()),
      ..Default::default()
    };
    sync_to_settings(&db, "/repo/a", &config).expect("sync should succeed");

    assert_eq!(
      db.get_repo_setting("/repo/a", "default_model").unwrap(),
      Some("opus".to_string())
    );
  }
}

#[cfg(test)]
mod proptests {
  use super::*;
  use crate::test_repo::proptest_config;
  use proptest::prelude::*;

  fn value() -> impl Strategy<Value = Option<String>> {
    // YAML-hostile text: colons, quotes, `#`, leading dashes, `null`, `yes`.
    proptest::option::of(prop_oneof![
      "[ -~]{0,20}",
      Just("null".to_string()),
      Just("~".to_string()),
      Just("yes".to_string()),
      Just("0x1F".to_string()),
      "\\PC{0,8}",
      "[a-z]{0,4}\n[a-z]{0,4}",
    ])
  }

  fn config() -> impl Strategy<Value = RepoConfig> {
    (
      (value(), value(), value(), value()),
      proptest::option::of(prop::collection::vec("[ -~]{0,10}", 0..4)),
      (value(), value(), value(), value()),
    )
      .prop_map(|((a, b, c, d), files, (e, f, g, h))| RepoConfig {
        branch_name_pattern: a,
        default_model: b,
        default_agent: c,
        target_branch: d,
        included_copy_files: files,
        setup_script: e,
        review_prompt: f,
        review_agent: g,
        auto_review_trigger: h,
      })
  }

  proptest! {
    #![proptest_config(proptest_config(64))]

    /// Any config written as YAML to `.treq/config.yaml` reads back equal.
    #[test]
    fn config_round_trips_through_the_yaml_file(config in config()) {
      let dir = tempfile::TempDir::new().map_err(|e| TestCaseError::fail(e.to_string()))?;
      let repo = dir.path().to_string_lossy().to_string();
      let path = config_path(&repo);
      std::fs::create_dir_all(path.parent().expect("has parent")).map_err(|e| TestCaseError::fail(e.to_string()))?;
      std::fs::write(&path, serde_yaml::to_string(&config).map_err(|e| TestCaseError::fail(e.to_string()))?)
        .map_err(|e| TestCaseError::fail(e.to_string()))?;
      prop_assert_eq!(parse_config(&repo), Ok(config));
    }

    /// Arbitrary file contents give `Ok` or `Err`, never a panic.
    #[test]
    fn parse_config_never_panics(content in "\\PC{0,60}") {
      let dir = tempfile::TempDir::new().map_err(|e| TestCaseError::fail(e.to_string()))?;
      let repo = dir.path().to_string_lossy().to_string();
      let path = config_path(&repo);
      std::fs::create_dir_all(path.parent().expect("has parent")).map_err(|e| TestCaseError::fail(e.to_string()))?;
      std::fs::write(&path, content).map_err(|e| TestCaseError::fail(e.to_string()))?;
      let _ = parse_config(&repo);
    }
  }
}
