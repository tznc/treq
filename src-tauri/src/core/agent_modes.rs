//! Modes of an agent account profile: named presets, such as `deep` and
//! `fast` for Claude's `work` profile, that start the agent with its own
//! model, effort, permission mode and extra instructions on the same
//! account. Each profile can mark one mode as its default.

use serde::{Deserialize, Serialize};

use super::agent_profiles::validate_name;

/// The app setting holding every profile's modes, as JSON.
pub const MODES_SETTING: &str = "agent_modes";

/// The permission modes a mode can start in.
pub const PERMISSION_MODES: [&str; 2] = ["acceptEdits", "plan"];

/// One preset of a profile. Unset fields keep the agent's own default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Mode {
  pub name: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub model: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub effort: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub permission_mode: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub instructions: Option<String>,
}

/// The modes of one profile, and which one it starts with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct ProfileModes {
  pub agent: String,
  pub profile: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub default: Option<String>,
  pub modes: Vec<Mode>,
}

/// Every profile's modes, as stored in [`MODES_SETTING`].
pub type Modes = Vec<ProfileModes>;

/// The modes stored in [`MODES_SETTING`]; nothing when unset or invalid.
pub fn parse(json: Option<&str>) -> Modes {
  json
    .and_then(|raw| serde_json::from_str(raw).ok())
    .unwrap_or_default()
}

pub fn to_json(modes: &Modes) -> String {
  serde_json::to_string(modes).unwrap_or_else(|_| "[]".into())
}

/// `agent`'s `profile`'s modes, in the order they were added.
pub fn modes_for<'a>(all: &'a Modes, agent: &str, profile: &str) -> &'a [Mode] {
  find(all, agent, profile).map_or(&[], |p| p.modes.as_slice())
}

fn find<'a>(all: &'a Modes, agent: &str, profile: &str) -> Option<&'a ProfileModes> {
  all
    .iter()
    .find(|p| p.agent == agent && p.profile == profile)
}

/// `all` with the profile's entry changed by `edit`, created if missing.
fn with_profile(
  all: &Modes,
  agent: &str,
  profile: &str,
  edit: impl FnOnce(&mut ProfileModes) -> Result<(), String>,
) -> Result<Modes, String> {
  let mut all = all.clone();
  let index = match all
    .iter()
    .position(|p| p.agent == agent && p.profile == profile)
  {
    Some(index) => index,
    None => {
      all.push(ProfileModes {
        agent: agent.into(),
        profile: profile.into(),
        ..Default::default()
      });
      all.len() - 1
    }
  };
  edit(&mut all[index])?;
  all.retain(|p| !p.modes.is_empty());
  Ok(all)
}

/// `None` for a blank value.
fn set(value: Option<String>) -> Option<String> {
  value
    .map(|v| v.trim().to_string())
    .filter(|v| !v.is_empty())
}

/// `all` with `mode` saved for the profile: added, or replacing the mode
/// of the same name. Names follow profile names; empty fields are unset.
pub fn save(all: &Modes, agent: &str, profile: &str, mode: Mode) -> Result<Modes, String> {
  let mode = Mode {
    name: validate_name(&mode.name)?,
    model: set(mode.model),
    effort: set(mode.effort),
    permission_mode: set(mode.permission_mode),
    instructions: set(mode.instructions),
  };
  if let Some(permission) = &mode.permission_mode {
    if !PERMISSION_MODES.contains(&permission.as_str()) {
      return Err(format!("Unknown permission mode {permission}"));
    }
  }
  with_profile(all, agent, profile, |p| {
    match p.modes.iter_mut().find(|m| m.name == mode.name) {
      Some(existing) => *existing = mode,
      None => p.modes.push(mode),
    }
    Ok(())
  })
}

/// `all` without the profile's mode `name`; it stops being the default.
pub fn remove(all: &Modes, agent: &str, profile: &str, name: &str) -> Modes {
  with_profile(all, agent, profile, |p| {
    p.modes.retain(|m| m.name != name);
    if p.default.as_deref() == Some(name) {
      p.default = None;
    }
    Ok(())
  })
  .unwrap_or_else(|_| all.clone())
}

/// `all` with the profile's default mode set, or cleared with `None`.
pub fn set_default(
  all: &Modes,
  agent: &str,
  profile: &str,
  name: Option<&str>,
) -> Result<Modes, String> {
  if let Some(name) = name {
    if !modes_for(all, agent, profile)
      .iter()
      .any(|m| m.name == name)
    {
      return Err(format!("{profile} has no mode named {name}"));
    }
  }
  with_profile(all, agent, profile, |p| {
    p.default = name.map(str::to_string);
    Ok(())
  })
}

/// `all` without any of a removed profile's modes.
pub fn remove_profile(all: &Modes, agent: &str, profile: &str) -> Modes {
  all
    .iter()
    .filter(|p| !(p.agent == agent && p.profile == profile))
    .cloned()
    .collect()
}

/// The mode a session on the profile starts with: the chosen one, else the
/// profile's default. `None` keeps the agent's own settings.
pub fn resolve(all: &Modes, agent: &str, profile: &str, chosen: Option<&str>) -> Option<Mode> {
  let entry = find(all, agent, profile)?;
  [chosen, entry.default.as_deref()]
    .into_iter()
    .flatten()
    .find_map(|name| entry.modes.iter().find(|m| m.name == name).cloned())
}

/// `profile` or `profile/mode`, as `@work/deep` and `--profile work/deep`
/// write it.
pub fn split_target(target: &str) -> (&str, Option<&str>) {
  match target.split_once('/') {
    Some((profile, mode)) => (profile, Some(mode).filter(|m| !m.is_empty())),
    None => (target, None),
  }
}

/// Typed launch arguments that start `agent` in `mode` (remote hosts); empty without modes.
pub fn cli_args(agent: &str, mode: &Mode) -> Vec<String> {
  let set = |flag: &str, value: &Option<String>| {
    value
      .iter()
      .flat_map(|v| [flag.to_string(), v.clone()])
      .collect::<Vec<_>>()
  };
  let config = |key: &str, value: &Option<String>| {
    value
      .iter()
      .flat_map(|v| ["-c".to_string(), format!("{key}={v}")])
      .collect::<Vec<_>>()
  };
  match agent {
    "claude" => [
      set("--model", &mode.model),
      set("--effort", &mode.effort),
      set("--permission-mode", &mode.permission_mode),
      set("--append-system-prompt", &mode.instructions),
    ]
    .concat(),
    "codex" => [
      set("-m", &mode.model),
      config("model_reasoning_effort", &mode.effort),
      config("developer_instructions", &mode.instructions),
    ]
    .concat(),
    _ => Vec::new(),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn mode(name: &str) -> Mode {
    Mode {
      name: name.into(),
      ..Default::default()
    }
  }

  fn deep() -> Mode {
    Mode {
      name: "deep".into(),
      model: Some("opus".into()),
      effort: Some("high".into()),
      permission_mode: Some("plan".into()),
      instructions: Some("Think it through.".into()),
    }
  }

  #[test]
  fn round_trips_through_json_and_drops_bad_json() {
    let all = save(&Vec::new(), "claude", "work", deep()).unwrap();
    assert_eq!(parse(Some(&to_json(&all))), all);
    assert!(parse(None).is_empty());
    assert!(parse(Some("{not json")).is_empty());
    // Unset fields stay out of the stored JSON.
    let plain = save(&Vec::new(), "claude", "work", mode("fast")).unwrap();
    assert_eq!(
      to_json(&plain),
      r#"[{"agent":"claude","profile":"work","modes":[{"name":"fast"}]}]"#
    );
  }

  #[test]
  fn saves_modes_per_profile() {
    let all = save(&Vec::new(), "claude", "work", deep()).unwrap();
    let all = save(&all, "claude", "work", mode("fast")).unwrap();
    let all = save(&all, "claude", "personal", mode("fast")).unwrap();
    let names = |p: &str| -> Vec<String> {
      modes_for(&all, "claude", p)
        .iter()
        .map(|m| m.name.clone())
        .collect()
    };
    assert_eq!(names("work"), ["deep", "fast"]);
    assert_eq!(names("personal"), ["fast"]);
    assert!(modes_for(&all, "codex", "work").is_empty());
  }

  #[test]
  fn saving_a_mode_of_the_same_name_replaces_it() {
    let all = save(&Vec::new(), "claude", "work", deep()).unwrap();
    let edited = Mode {
      model: Some("sonnet".into()),
      ..deep()
    };
    let all = save(&all, "claude", "work", edited.clone()).unwrap();
    assert_eq!(modes_for(&all, "claude", "work"), [edited]);
  }

  #[test]
  fn blank_fields_are_unset_and_bad_values_refused() {
    let blank = Mode {
      name: " fast ".into(),
      model: Some("  ".into()),
      instructions: Some("".into()),
      ..Default::default()
    };
    let all = save(&Vec::new(), "claude", "work", blank).unwrap();
    assert_eq!(modes_for(&all, "claude", "work"), [mode("fast")]);
    assert!(save(&all, "claude", "work", mode("Deep Work")).is_err());
    let bad_permission = Mode {
      permission_mode: Some("yolo".into()),
      ..mode("x")
    };
    assert!(save(&all, "claude", "work", bad_permission).is_err());
  }

  #[test]
  fn removing_the_default_mode_clears_the_default() {
    let all = save(&Vec::new(), "claude", "work", deep()).unwrap();
    let all = save(&all, "claude", "work", mode("fast")).unwrap();
    let all = set_default(&all, "claude", "work", Some("deep")).unwrap();
    let all = remove(&all, "claude", "work", "deep");
    assert_eq!(modes_for(&all, "claude", "work"), [mode("fast")]);
    assert_eq!(resolve(&all, "claude", "work", None), None);
  }

  #[test]
  fn the_default_must_be_one_of_the_profiles_modes() {
    let all = save(&Vec::new(), "claude", "work", deep()).unwrap();
    assert!(set_default(&all, "claude", "work", Some("fast")).is_err());
    let all = set_default(&all, "claude", "work", Some("deep")).unwrap();
    let cleared = set_default(&all, "claude", "work", None).unwrap();
    assert_eq!(resolve(&cleared, "claude", "work", None), None);
  }

  #[test]
  fn resolves_the_chosen_mode_then_the_default() {
    let all = save(&Vec::new(), "claude", "work", deep()).unwrap();
    let all = save(&all, "claude", "work", mode("fast")).unwrap();
    let all = set_default(&all, "claude", "work", Some("fast")).unwrap();
    assert_eq!(resolve(&all, "claude", "work", Some("deep")), Some(deep()));
    assert_eq!(resolve(&all, "claude", "work", None), Some(mode("fast")));
    // A mode the profile doesn't have falls back to its default.
    assert_eq!(
      resolve(&all, "claude", "work", Some("gone")),
      Some(mode("fast"))
    );
    assert_eq!(resolve(&all, "claude", "personal", Some("deep")), None);
  }

  #[test]
  fn removing_a_profile_drops_its_modes() {
    let all = save(&Vec::new(), "claude", "work", deep()).unwrap();
    let all = save(&all, "claude", "personal", mode("fast")).unwrap();
    let all = remove_profile(&all, "claude", "work");
    assert!(modes_for(&all, "claude", "work").is_empty());
    assert_eq!(modes_for(&all, "claude", "personal"), [mode("fast")]);
  }

  #[test]
  fn splits_profile_and_mode() {
    assert_eq!(split_target("work"), ("work", None));
    assert_eq!(split_target("work/deep"), ("work", Some("deep")));
    assert_eq!(split_target("work/"), ("work", None));
  }

  #[test]
  fn turns_a_mode_into_claude_and_codex_arguments() {
    assert_eq!(
      cli_args("claude", &deep()),
      [
        "--model",
        "opus",
        "--effort",
        "high",
        "--permission-mode",
        "plan",
        "--append-system-prompt",
        "Think it through.",
      ]
    );
    assert_eq!(
      cli_args("codex", &deep()),
      [
        "-m",
        "opus",
        "-c",
        "model_reasoning_effort=high",
        "-c",
        "developer_instructions=Think it through.",
      ]
    );
    assert!(cli_args("claude", &mode("plain")).is_empty());
    assert!(cli_args("gemini", &deep()).is_empty());
  }
}
