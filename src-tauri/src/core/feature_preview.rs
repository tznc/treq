use serde_json::Value;
use std::sync::OnceLock;

use crate::db::Database;

const PACKAGE_JSON: &str = include_str!("../../../package.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewFeature {
  SkillsInstallation,
  RemoteSsh,
  LinearIntegration,
  Logs,
  Devcontainers,
  /// Bring your own sandbox: cloud workspaces on the user's own provider
  /// account.
  Byos,
}

impl PreviewFeature {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::SkillsInstallation => "skillsInstallation",
      Self::RemoteSsh => "remoteSsh",
      Self::LinearIntegration => "linearIntegration",
      Self::Logs => "logs",
      Self::Devcontainers => "devcontainers",
      Self::Byos => "byos",
    }
  }

  pub fn setting_key(self) -> String {
    format!("feature_preview.{}", self.as_str())
  }

  pub fn disabled_message(self) -> String {
    format!("Feature preview '{}' is disabled", self.as_str())
  }
}

fn package_flag(id: &str) -> bool {
  static FLAGS: OnceLock<Value> = OnceLock::new();
  let flags = FLAGS.get_or_init(|| {
    serde_json::from_str::<Value>(PACKAGE_JSON)
      .ok()
      .and_then(|pkg| pkg.get("featureFlags").cloned())
      .unwrap_or(Value::Null)
  });
  flags.get(id).and_then(Value::as_bool).unwrap_or(false)
}

pub fn compile_default(feature: PreviewFeature) -> bool {
  // Dev builds turn every preview on.
  if cfg!(debug_assertions) {
    true
  } else {
    package_flag(feature.as_str())
  }
}

pub fn package_json_default(feature: PreviewFeature) -> bool {
  package_flag(feature.as_str())
}

pub fn is_enabled(db: &Database, feature: PreviewFeature) -> bool {
  match db.get_setting(&feature.setting_key()) {
    Ok(Some(value)) if value == "true" => true,
    Ok(Some(value)) if value == "false" => false,
    _ => compile_default(feature),
  }
}

pub fn is_enabled_in_app_db(feature: PreviewFeature) -> bool {
  let path = crate::core::resolve_app_db_path("");
  match Database::new(path) {
    Ok(db) => is_enabled(&db, feature),
    Err(_) => compile_default(feature),
  }
}

pub fn require(db: &Database, feature: PreviewFeature) -> Result<(), String> {
  if is_enabled(db, feature) {
    Ok(())
  } else {
    Err(feature.disabled_message())
  }
}

/// Gate for commands that act on a specific SSH endpoint. Treq-managed cloud
/// workspaces (`Managed`) are always allowed: server-side Pro enforcement
/// already gates them. User-managed and alias endpoints stay behind the
/// `remoteSsh` preview.
pub fn require_remote_for_source(
  db: &Database,
  source: &crate::core::remote_control_plane::SshEndpointSource,
) -> Result<(), String> {
  use crate::core::remote_control_plane::SshEndpointSource;
  match source {
    SshEndpointSource::Managed { .. } => Ok(()),
    SshEndpointSource::UserManaged | SshEndpointSource::ExplicitAlias { .. } => {
      require(db, PreviewFeature::RemoteSsh)
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::db::Database;
  use tempfile::TempDir;

  fn temp_db() -> (TempDir, Database) {
    let dir = TempDir::new().expect("tempdir");
    let db = Database::new(dir.path().join("treq.db")).expect("db");
    db.init().expect("init");
    (dir, db)
  }

  #[test]
  fn package_json_preview_flags_default_off() {
    assert!(!package_json_default(PreviewFeature::SkillsInstallation));
    assert!(!package_json_default(PreviewFeature::RemoteSsh));
    assert!(!package_json_default(PreviewFeature::LinearIntegration));
    assert!(!package_json_default(PreviewFeature::Byos));
  }

  #[test]
  fn package_json_enables_logs_by_default() {
    assert!(package_json_default(PreviewFeature::Logs));
  }

  #[test]
  fn stored_false_overrides_debug_default() {
    let (_dir, db) = temp_db();
    db.set_setting(&PreviewFeature::RemoteSsh.setting_key(), "false")
      .expect("set");
    assert!(!is_enabled(&db, PreviewFeature::RemoteSsh));
  }

  #[test]
  fn stored_true_enables_feature() {
    let (_dir, db) = temp_db();
    db.set_setting(&PreviewFeature::SkillsInstallation.setting_key(), "true")
      .expect("set");
    assert!(is_enabled(&db, PreviewFeature::SkillsInstallation));
  }

  #[test]
  fn require_errors_when_disabled() {
    let (_dir, db) = temp_db();
    db.set_setting(&PreviewFeature::RemoteSsh.setting_key(), "false")
      .expect("set");
    let err = require(&db, PreviewFeature::RemoteSsh).expect_err("disabled");
    assert!(err.contains("remoteSsh"));
  }

  #[test]
  fn managed_endpoint_passes_with_remote_ssh_off() {
    use crate::core::remote_control_plane::SshEndpointSource;
    let (_dir, db) = temp_db();
    db.set_setting(&PreviewFeature::RemoteSsh.setting_key(), "false")
      .expect("set");
    let managed = SshEndpointSource::Managed {
      provider: "boat".into(),
      generation: 1,
    };
    assert!(require_remote_for_source(&db, &managed).is_ok());
  }

  #[test]
  fn user_managed_endpoints_fail_with_remote_ssh_off() {
    use crate::core::remote_control_plane::SshEndpointSource;
    let (_dir, db) = temp_db();
    db.set_setting(&PreviewFeature::RemoteSsh.setting_key(), "false")
      .expect("set");
    let err = require_remote_for_source(&db, &SshEndpointSource::UserManaged).expect_err("gated");
    assert!(err.contains("remoteSsh"));
    let alias = SshEndpointSource::ExplicitAlias {
      alias: "box".into(),
    };
    assert!(require_remote_for_source(&db, &alias).is_err());
  }

  #[test]
  fn user_managed_endpoint_passes_with_remote_ssh_on() {
    use crate::core::remote_control_plane::SshEndpointSource;
    let (_dir, db) = temp_db();
    db.set_setting(&PreviewFeature::RemoteSsh.setting_key(), "true")
      .expect("set");
    assert!(require_remote_for_source(&db, &SshEndpointSource::UserManaged).is_ok());
  }
}
