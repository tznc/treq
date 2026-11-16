//! Named account profiles for an agent CLI, such as `work` and `personal`
//! for Claude. Each profile has its own config directory under the app data
//! directory, which the agent reads through its config-dir variable, so each
//! one signs in to its own account.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The app setting holding every profile, as JSON.
pub const PROFILES_SETTING: &str = "agent_profiles";

/// One named account of an agent CLI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentProfile {
  pub agent: String,
  pub name: String,
}

/// The variable an agent reads its config directory from; `None` when the
/// agent has no profiles.
fn config_dir_var(agent: &str) -> Option<&'static str> {
  match agent {
    "claude" => Some("CLAUDE_CONFIG_DIR"),
    "codex" => Some("CODEX_HOME"),
    _ => None,
  }
}

pub fn supports_profiles(agent: &str) -> bool {
  config_dir_var(agent).is_some()
}

/// The repository and app setting naming an agent's default profile.
pub fn default_setting(agent: &str) -> String {
  format!("default_profile_{agent}")
}

/// A profile name: 1 to 32 lowercase letters, digits, `-` or `_`.
pub fn validate_name(name: &str) -> Result<String, String> {
  todo!()
}

/// The profiles stored in [`PROFILES_SETTING`]; nothing when unset or invalid.
pub fn parse(json: Option<&str>) -> Vec<AgentProfile> {
  todo!()
}

pub fn to_json(profiles: &[AgentProfile]) -> String {
  todo!()
}

/// `profiles` with a new one added.
pub fn add(profiles: &[AgentProfile], agent: &str, name: &str) -> Result<Vec<AgentProfile>, String> {
  todo!()
}

/// `profiles` without `agent`'s `name`.
pub fn remove(profiles: &[AgentProfile], agent: &str, name: &str) -> Vec<AgentProfile> {
  todo!()
}

/// The profile's config directory under the app data directory.
pub fn config_dir(app_data_dir: &Path, profile: &AgentProfile) -> PathBuf {
  todo!()
}

/// What the agent's command exports to use the profile.
pub fn env(app_data_dir: &Path, profile: &AgentProfile) -> Vec<(String, String)> {
  todo!()
}

/// The profile a session of `agent` starts with: the chosen one, else the
/// repository's default, else the app's. `None` uses the agent's own config.
/// A name with no matching profile is skipped.
pub fn resolve(
  profiles: &[AgentProfile],
  agent: &str,
  chosen: Option<&str>,
  repo_default: Option<&str>,
  app_default: Option<&str>,
) -> Option<AgentProfile> {
  todo!()
}

#[cfg(test)]
mod tests {
  use super::*;

  fn profile(agent: &str, name: &str) -> AgentProfile {
    AgentProfile {
      agent: agent.into(),
      name: name.into(),
    }
  }

  #[test]
  fn only_claude_and_codex_support_profiles() {
    assert!(supports_profiles("claude"));
    assert!(supports_profiles("codex"));
    assert!(!supports_profiles("cursor"));
  }

  #[test]
  fn validates_names() {
    assert_eq!(validate_name(" work ").unwrap(), "work");
    assert_eq!(validate_name("side_job-2").unwrap(), "side_job-2");
    for bad in ["", "Work", "a b", "../x", &"a".repeat(33)] {
      assert!(validate_name(bad).is_err(), "{bad:?} should be refused");
    }
  }

  #[test]
  fn round_trips_through_json() {
    let profiles = vec![profile("claude", "work"), profile("codex", "personal")];
    assert_eq!(parse(Some(&to_json(&profiles))), profiles);
    assert!(parse(None).is_empty());
    assert!(parse(Some("not json")).is_empty());
  }

  #[test]
  fn adds_a_profile() {
    let added = add(&[], "claude", "work").unwrap();
    assert_eq!(added, vec![profile("claude", "work")]);
    let both = add(&added, "codex", "work").unwrap();
    assert_eq!(both.len(), 2);
  }

  #[test]
  fn refuses_duplicates_unsupported_agents_and_bad_names() {
    let existing = vec![profile("claude", "work")];
    assert!(add(&existing, "claude", "work").is_err());
    assert!(add(&existing, "cursor", "work").is_err());
    assert!(add(&existing, "claude", "Bad Name").is_err());
  }

  #[test]
  fn removes_a_profile() {
    let profiles = vec![profile("claude", "work"), profile("codex", "work")];
    assert_eq!(
      remove(&profiles, "claude", "work"),
      vec![profile("codex", "work")]
    );
  }

  #[test]
  fn config_dir_is_under_the_app_data_dir() {
    let dir = config_dir(Path::new("/data"), &profile("claude", "work"));
    assert_eq!(dir, Path::new("/data/agent-profiles/claude/work"));
  }

  #[test]
  fn env_points_the_agent_at_the_profile_dir() {
    let data = Path::new("/data");
    assert_eq!(
      env(data, &profile("claude", "work")),
      vec![(
        "CLAUDE_CONFIG_DIR".to_string(),
        "/data/agent-profiles/claude/work".to_string()
      )]
    );
    assert_eq!(
      env(data, &profile("codex", "personal"))[0].0,
      "CODEX_HOME"
    );
  }

  #[test]
  fn resolves_chosen_then_repo_then_app_default() {
    let profiles = vec![
      profile("claude", "work"),
      profile("claude", "personal"),
      profile("codex", "work"),
    ];
    let resolve = |chosen, repo, app| resolve(&profiles, "claude", chosen, repo, app);
    assert_eq!(
      resolve(Some("personal"), Some("work"), Some("work")),
      Some(profile("claude", "personal"))
    );
    assert_eq!(
      resolve(None, Some("work"), Some("personal")),
      Some(profile("claude", "work"))
    );
    assert_eq!(
      resolve(None, None, Some("personal")),
      Some(profile("claude", "personal"))
    );
    assert_eq!(resolve(None, None, None), None);
  }

  #[test]
  fn resolve_skips_missing_and_empty_names() {
    let profiles = vec![profile("claude", "work"), profile("codex", "solo")];
    assert_eq!(
      resolve(&profiles, "claude", Some(""), Some("gone"), Some("work")),
      Some(profile("claude", "work"))
    );
    // Another agent's profile never applies.
    assert_eq!(resolve(&profiles, "claude", Some("solo"), None, None), None);
  }
}
