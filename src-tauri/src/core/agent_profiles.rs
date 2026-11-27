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

/// A profile name: 1 to 32 lowercase letters, digits, `-` or `_`,
/// starting with a letter or digit so it never reads as a flag.
pub fn validate_name(name: &str) -> Result<String, String> {
  let name = name.trim();
  let word = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit();
  let valid = (1..=32).contains(&name.len())
    && name.starts_with(word)
    && name.chars().all(|c| word(c) || c == '-' || c == '_');
  if valid {
    Ok(name.to_string())
  } else {
    Err("Use 1 to 32 lowercase letters, digits, - or _, starting with a letter or digit".into())
  }
}

/// The profiles stored in [`PROFILES_SETTING`]; nothing when unset or invalid.
pub fn parse(json: Option<&str>) -> Vec<AgentProfile> {
  json
    .and_then(|raw| serde_json::from_str(raw).ok())
    .unwrap_or_default()
}

pub fn to_json(profiles: &[AgentProfile]) -> String {
  serde_json::to_string(profiles).unwrap_or_else(|_| "[]".into())
}

/// `profiles` with a new one added.
pub fn add(
  profiles: &[AgentProfile],
  agent: &str,
  name: &str,
) -> Result<Vec<AgentProfile>, String> {
  if !supports_profiles(agent) {
    return Err(format!("{agent} does not support profiles"));
  }
  let name = validate_name(name)?;
  if profiles.iter().any(|p| p.agent == agent && p.name == name) {
    return Err(format!("{agent} already has a profile named {name}"));
  }
  let mut added = profiles.to_vec();
  added.push(AgentProfile {
    agent: agent.into(),
    name,
  });
  Ok(added)
}

/// `profiles` without `agent`'s `name`.
pub fn remove(profiles: &[AgentProfile], agent: &str, name: &str) -> Vec<AgentProfile> {
  profiles
    .iter()
    .filter(|p| !(p.agent == agent && p.name == name))
    .cloned()
    .collect()
}

/// The profile's config directory under the app data directory.
pub fn config_dir(app_data_dir: &Path, profile: &AgentProfile) -> PathBuf {
  app_data_dir
    .join("agent-profiles")
    .join(&profile.agent)
    .join(&profile.name)
}

/// What the agent's command exports to use the profile.
pub fn env(app_data_dir: &Path, profile: &AgentProfile) -> Vec<(String, String)> {
  config_dir_var(&profile.agent)
    .map(|var| {
      let dir = config_dir(app_data_dir, profile);
      (var.to_string(), dir.to_string_lossy().into_owned())
    })
    .into_iter()
    .collect()
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
  [chosen, repo_default, app_default]
    .into_iter()
    .flatten()
    .filter(|name| !name.is_empty())
    .find_map(|name| {
      profiles
        .iter()
        .find(|p| p.agent == agent && p.name == name)
        .cloned()
    })
}

/// [`resolve`] from settings: the profiles and app default from the app
/// settings, the repository default from the repository's.
pub fn session_profile(
  app_setting: impl Fn(&str) -> Option<String>,
  repo_setting: impl Fn(&str) -> Option<String>,
  agent: &str,
  chosen: Option<&str>,
) -> Option<AgentProfile> {
  let key = default_setting(agent);
  resolve(
    &parse(app_setting(PROFILES_SETTING).as_deref()),
    agent,
    chosen,
    repo_setting(&key).as_deref(),
    app_setting(&key).as_deref(),
  )
}

/// A leading `@name` in a prompt: the profile it names, preferring
/// `agent`'s, and the prompt after it. `None` when no single profile has
/// that name, so other `@` text is left alone.
pub fn mention<'a>(
  profiles: &[AgentProfile],
  agent: &str,
  prompt: &'a str,
) -> Option<(AgentProfile, &'a str)> {
  let rest = prompt.trim_start().strip_prefix('@')?;
  let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
  let (name, rest) = rest.split_at(end);
  let named: Vec<_> = profiles.iter().filter(|p| p.name == name).collect();
  let found = named
    .iter()
    .find(|p| p.agent == agent)
    .or_else(|| (named.len() == 1).then(|| &named[0]))?;
  Some(((*found).clone(), rest.trim_start()))
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
    for bad in [
      "",
      "Work",
      "a b",
      "../x",
      &"a".repeat(33),
      "-",
      "_",
      "-p",
      "_work",
    ] {
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
    assert_eq!(env(data, &profile("codex", "personal"))[0].0, "CODEX_HOME");
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

  #[test]
  fn session_profile_reads_profiles_and_defaults_from_settings() {
    let json = to_json(&[profile("claude", "work"), profile("claude", "personal")]);
    let app = |key: &str| match key {
      PROFILES_SETTING => Some(json.clone()),
      "default_profile_claude" => Some("personal".to_string()),
      _ => None,
    };
    let repo = |key: &str| (key == "default_profile_claude").then(|| "work".to_string());
    assert_eq!(
      session_profile(app, repo, "claude", None),
      Some(profile("claude", "work"))
    );
    assert_eq!(
      session_profile(app, |_| None, "claude", None),
      Some(profile("claude", "personal"))
    );
    assert_eq!(session_profile(app, repo, "codex", None), None);
  }

  #[test]
  fn mention_routes_to_the_named_profile() {
    let profiles = vec![
      profile("claude", "work"),
      profile("codex", "work"),
      profile("codex", "personal"),
    ];
    assert_eq!(
      mention(&profiles, "claude", "  @work fix the bug"),
      Some((profile("claude", "work"), "fix the bug"))
    );
    assert_eq!(
      mention(&profiles, "codex", "@work fix"),
      Some((profile("codex", "work"), "fix"))
    );
    // Only Codex has it: the prompt goes to Codex.
    assert_eq!(
      mention(&profiles, "claude", "@personal\nfix"),
      Some((profile("codex", "personal"), "fix"))
    );
  }

  #[test]
  fn mention_leaves_other_prompts_alone() {
    let profiles = vec![profile("claude", "work"), profile("codex", "work")];
    assert_eq!(mention(&profiles, "claude", "fix @work later"), None);
    assert_eq!(mention(&profiles, "claude", "@src/main.rs is broken"), None);
    // Two other agents share the name: ambiguous.
    assert_eq!(mention(&profiles, "gemini", "@work fix"), None);
  }
}
