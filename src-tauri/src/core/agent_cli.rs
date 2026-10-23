use crate::result_ext::ResultExt;
use serde::Serialize;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

const FILE_PREFIX: &str = "treq-agent-";
const TREQ_SKILL_MD: &str = include_str!("../../resources/agent-skill/SKILL.md");
const PROJECT_SKILL_MARKER: &str = ".treq-generated";
const AGENTS_SKILL_RELATIVE: &str = ".agents/skills/treq";
const CLAUDE_SKILL_RELATIVE: &str = ".claude/skills/treq";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCliFiles {
  pub prompt_path: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub settings_path: Option<String>,
  pub skill_dir: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub agents_skill_path: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub claude_skill_path: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub skill_write_warning: Option<String>,
}

/// Write the agent system prompt, optional Claude settings, and bundled Treq skill pack.
pub fn write_agent_cli_files(
  prompt: &str,
  settings_json: Option<&str>,
  cwd: Option<&str>,
) -> Result<AgentCliFiles, String> {
  let id = uuid::Uuid::new_v4();
  let dir = std::env::temp_dir();

  let skill_dir = write_treq_skill_pack(&dir, &id)?;
  let mut skill_write_warnings = Vec::new();
  let (agents_skill_path, claude_skill_path) = match cwd {
    Some(path) => {
      let cwd = Path::new(path);
      (
        install_project_treq_skill(cwd, AGENTS_SKILL_RELATIVE, &mut skill_write_warnings),
        install_project_treq_skill(cwd, CLAUDE_SKILL_RELATIVE, &mut skill_write_warnings),
      )
    }
    None => (None, None),
  };

  let prompt_with_skill = format!(
    "{prompt} The bundled Treq skill file is at {}/skills/treq/SKILL.md.",
    skill_dir.to_string_lossy()
  );
  let prompt_path = dir.join(format!("{FILE_PREFIX}prompt-{id}.txt"));
  fs::write(&prompt_path, prompt_with_skill)
    .map_err(|e| format!("Failed to write agent prompt file: {e}"))?;

  let settings_path = match settings_json {
    Some(json) => {
      let merged = with_skill_dir_filesystem_access(json, &skill_dir)?;
      let path = dir.join(format!("{FILE_PREFIX}settings-{id}.json"));
      fs::write(&path, merged).map_err(|e| format!("Failed to write agent settings file: {e}"))?;
      Some(path_to_string(&path))
    }
    None => None,
  };

  Ok(AgentCliFiles {
    prompt_path: path_to_string(&prompt_path),
    settings_path,
    skill_dir: path_to_string(&skill_dir),
    agents_skill_path,
    claude_skill_path,
    skill_write_warning: if skill_write_warnings.is_empty() {
      None
    } else {
      Some(skill_write_warnings.join(" "))
    },
  })
}

/// Delete temp files previously created by [`write_agent_cli_files`].
/// Refuses paths that are not `treq-agent-*` files or dirs under the process temp dir.
pub fn cleanup_agent_cli_files(paths: &[String]) -> Result<(), String> {
  let temp_dir = std::env::temp_dir()
    .canonicalize()
    .map_err(|e| format!("Failed to resolve temp dir: {e}"))?;

  for path in paths {
    if path.is_empty() {
      continue;
    }
    let candidate = Path::new(path);
    let Ok(canonical) = candidate.canonicalize() else {
      continue;
    };
    if !is_safe_agent_cli_temp_path(&canonical, &temp_dir) && !is_safe_project_skill_dir(&canonical)
    {
      return Err(format!(
        "Refusing to delete path outside treq agent temp files: {path}"
      ));
    }
    if canonical.is_dir() {
      fs::remove_dir_all(&canonical).best_effort();
    } else {
      fs::remove_file(&canonical).best_effort();
    }
  }
  Ok(())
}

fn write_treq_skill_pack(temp_dir: &Path, id: &uuid::Uuid) -> Result<PathBuf, String> {
  let skill_dir = temp_dir.join(format!("{FILE_PREFIX}skills-{id}"));
  let copies = [
    skill_dir.join("skills/treq/SKILL.md"),
    skill_dir.join(".claude/skills/treq/SKILL.md"),
    skill_dir.join(".agents/skills/treq/SKILL.md"),
  ];
  for path in &copies {
    let parent = path
      .parent()
      .ok_or_else(|| "Invalid skill pack path".to_string())?;
    fs::create_dir_all(parent).map_err(|e| format!("Failed to create skill pack dir: {e}"))?;
    fs::write(path, TREQ_SKILL_MD).map_err(|e| format!("Failed to write Treq skill: {e}"))?;
  }
  Ok(skill_dir)
}

fn install_project_treq_skill(
  cwd: &Path,
  relative: &str,
  warnings: &mut Vec<String>,
) -> Option<String> {
  let skill_dir = cwd.join(relative);
  let marker = skill_dir.join(PROJECT_SKILL_MARKER);
  if skill_dir.exists() && !marker.exists() {
    return None;
  }
  if let Err(e) = fs::create_dir_all(&skill_dir) {
    warnings.push(format!("Failed to create {relative}: {e}"));
    return None;
  }
  if let Err(e) = fs::write(skill_dir.join("SKILL.md"), TREQ_SKILL_MD) {
    warnings.push(format!("Failed to write Treq skill at {relative}: {e}"));
    return None;
  }
  if let Err(e) = fs::write(&marker, "treq\n") {
    warnings.push(format!("Failed to write Treq skill marker: {e}"));
    return None;
  }
  if let Err(e) = super::ignore_generated_dir(&skill_dir) {
    warnings.push(format!("Failed to ignore Treq skill at {relative}: {e}"));
  }
  Some(path_to_string(&skill_dir))
}

fn is_safe_project_skill_dir(canonical: &Path) -> bool {
  let is_known_skill_dir = canonical.ends_with(Path::new(AGENTS_SKILL_RELATIVE))
    || canonical.ends_with(Path::new(CLAUDE_SKILL_RELATIVE));
  is_known_skill_dir && canonical.join(PROJECT_SKILL_MARKER).is_file()
}

fn with_skill_dir_filesystem_access(
  settings_json: &str,
  skill_dir: &Path,
) -> Result<String, String> {
  let mut value: Value = serde_json::from_str(settings_json)
    .map_err(|e| format!("Failed to parse agent settings JSON: {e}"))?;
  value.as_object_mut().and_then(|obj| obj.remove("sandbox"));
  let skill_dir = skill_dir.to_string_lossy().into_owned();
  let filesystem = value
    .pointer_mut("/filesystem")
    .and_then(Value::as_object_mut)
    .ok_or_else(|| "Agent settings JSON is missing filesystem".to_string())?;
  push_unique_path(filesystem, "allowRead", &skill_dir, false)?;
  push_unique_path(filesystem, "allowWrite", &skill_dir, true)?;
  serde_json::to_string_pretty(&value).map_err(|e| format!("Failed to serialize settings: {e}"))
}

fn push_unique_path(
  filesystem: &mut serde_json::Map<String, Value>,
  key: &str,
  path: &str,
  create: bool,
) -> Result<(), String> {
  match filesystem.get_mut(key).and_then(Value::as_array_mut) {
    Some(paths) => {
      if !paths.iter().any(|p| p.as_str() == Some(path)) {
        paths.push(json!(path));
      }
      Ok(())
    }
    None if create => {
      filesystem.insert(key.to_string(), json!([path]));
      Ok(())
    }
    None => Err(format!("Agent settings JSON is missing filesystem.{key}")),
  }
}

fn is_safe_agent_cli_temp_path(canonical: &Path, temp_dir: &Path) -> bool {
  let file_name = canonical.file_name().and_then(|n| n.to_str()).unwrap_or("");
  canonical.starts_with(temp_dir) && file_name.starts_with(FILE_PREFIX)
}

fn path_to_string(path: &Path) -> String {
  path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
  use super::*;
  use serde_json::Value;

  #[test]
  fn writes_prompt_settings_and_skill_pack() {
    let files = write_agent_cli_files(
      "you are in a workspace",
      Some(r#"{"filesystem":{"allowRead":["/ws"],"allowWrite":["/ws"]}}"#),
      None,
    )
    .expect("write files");

    let prompt = fs::read_to_string(&files.prompt_path).expect("read prompt");
    assert!(prompt.starts_with("you are in a workspace"));
    assert!(prompt.contains("/skills/treq/SKILL.md"));
    assert!(Path::new(&files.prompt_path)
      .file_name()
      .unwrap()
      .to_str()
      .unwrap()
      .starts_with("treq-agent-prompt-"));

    let settings_path = files.settings_path.expect("settings path");
    let settings = fs::read_to_string(&settings_path).expect("read settings");
    // skill_dir may contain backslashes (Windows), which JSON escapes as `\\`;
    // compare against the JSON-encoded form rather than the raw path string.
    let skill_dir_json = serde_json::to_string(&files.skill_dir).expect("encode skill_dir");
    assert!(settings.contains(skill_dir_json.trim_matches('"')));
    assert!(settings.contains("/ws"));
    assert!(!settings.contains("sandbox"));
    let value: Value = serde_json::from_str(&settings).expect("parse settings");
    assert!(value.get("sandbox").is_none());
    let allow_write = value
      .pointer("/filesystem/allowWrite")
      .and_then(Value::as_array)
      .unwrap();
    assert!(allow_write
      .iter()
      .any(|p| p.as_str() == Some(files.skill_dir.as_str())));

    let skill_md = Path::new(&files.skill_dir).join("skills/treq/SKILL.md");
    let body = fs::read_to_string(skill_md).expect("read skill");
    assert!(body.contains("name: treq"));
    assert!(body.contains("treq send"));
    assert!(body.contains("treq commit"));
    assert!(body.contains("treq add"));
    assert!(
      body.contains("Workspaces you create must be nested under the branch they are based on")
    );
    assert!(Path::new(&files.skill_dir)
      .join(".claude/skills/treq/SKILL.md")
      .exists());
    assert!(Path::new(&files.skill_dir)
      .join(".agents/skills/treq/SKILL.md")
      .exists());
    assert!(!Path::new(&files.skill_dir)
      .join(".cursor/skills/treq/SKILL.md")
      .exists());
    assert!(!Path::new(&files.skill_dir).join("plugin.json").exists());

    assert!(files.agents_skill_path.is_none());
    assert!(files.claude_skill_path.is_none());
    cleanup_agent_cli_files(&[files.prompt_path, settings_path, files.skill_dir]).expect("cleanup");
  }

  #[test]
  fn omits_settings_file_when_not_requested() {
    let files = write_agent_cli_files("prompt only", None, None).expect("write");
    assert!(files.settings_path.is_none());
    assert!(Path::new(&files.prompt_path).exists());
    assert!(Path::new(&files.skill_dir).is_dir());
    cleanup_agent_cli_files(&[files.prompt_path, files.skill_dir]).expect("cleanup");
  }

  #[test]
  fn cleanup_deletes_written_files_and_skill_dir() {
    let files = write_agent_cli_files("tmp", Some(r#"{"filesystem":{"allowRead":[]}}"#), None)
      .expect("write");
    let settings_path = files.settings_path.clone().unwrap();
    cleanup_agent_cli_files(&[
      files.prompt_path.clone(),
      settings_path.clone(),
      files.skill_dir.clone(),
    ])
    .expect("cleanup");
    assert!(!Path::new(&files.prompt_path).exists());
    assert!(!Path::new(&settings_path).exists());
    assert!(!Path::new(&files.skill_dir).exists());
  }

  #[test]
  fn cleanup_refuses_paths_outside_temp_prefix() {
    // Must exist on every platform: canonicalize() only rejects paths that don't
    // resolve; /etc/passwd doesn't exist on Windows, so it would be silently
    // skipped instead of exercising the temp-prefix check. The running test
    // binary always exists and is never under the treq-agent temp prefix.
    let outside_path = std::env::current_exe()
      .expect("current_exe")
      .to_string_lossy()
      .into_owned();
    let result = cleanup_agent_cli_files(&[outside_path]);
    assert!(result.is_err());
  }

  #[test]
  fn with_skill_dir_filesystem_access_appends_read_and_write() {
    let json =
      r#"{"filesystem":{"allowRead":["/ws"],"allowWrite":["/ws"]},"sandbox":{"enabled":true}}"#;
    let merged =
      with_skill_dir_filesystem_access(json, Path::new("/tmp/treq-agent-skills-1")).unwrap();
    let value: Value = serde_json::from_str(&merged).unwrap();
    assert!(value.get("sandbox").is_none());
    let allow_read = value
      .pointer("/filesystem/allowRead")
      .and_then(Value::as_array)
      .unwrap();
    let allow_write = value
      .pointer("/filesystem/allowWrite")
      .and_then(Value::as_array)
      .unwrap();
    assert_eq!(
      allow_read,
      &vec![json!("/ws"), json!("/tmp/treq-agent-skills-1")]
    );
    assert_eq!(
      allow_write,
      &vec![json!("/ws"), json!("/tmp/treq-agent-skills-1")]
    );
  }

  #[test]
  fn installs_project_skills_under_cwd_for_codex_and_claude_discovery() {
    let cwd = tempfile::TempDir::new().expect("cwd");
    let files = write_agent_cli_files("prompt", None, cwd.path().to_str()).expect("write");
    let agents_dir = files.agents_skill_path.expect("agents skill path");
    let claude_dir = files.claude_skill_path.expect("claude skill path");
    assert_eq!(
      Path::new(&agents_dir),
      cwd.path().join(".agents/skills/treq")
    );
    assert_eq!(
      Path::new(&claude_dir),
      cwd.path().join(".claude/skills/treq")
    );
    assert!(Path::new(&agents_dir).join("SKILL.md").exists());
    assert!(Path::new(&claude_dir).join("SKILL.md").exists());
    assert!(Path::new(&agents_dir).join(".treq-generated").exists());
    assert!(Path::new(&claude_dir).join(".treq-generated").exists());
    assert!(Path::new(&agents_dir).join(".gitignore").exists());
    assert!(Path::new(&claude_dir).join(".gitignore").exists());

    cleanup_agent_cli_files(&[
      files.prompt_path,
      files.skill_dir,
      agents_dir.clone(),
      claude_dir.clone(),
    ])
    .expect("cleanup");
    assert!(!Path::new(&agents_dir).exists());
    assert!(!Path::new(&claude_dir).exists());
  }

  #[test]
  fn does_not_overwrite_existing_user_project_skills() {
    let cwd = tempfile::TempDir::new().expect("cwd");
    let agents_dir = cwd.path().join(".agents/skills/treq");
    let claude_dir = cwd.path().join(".claude/skills/treq");
    fs::create_dir_all(&agents_dir).expect("mkdir agents");
    fs::create_dir_all(&claude_dir).expect("mkdir claude");
    fs::write(agents_dir.join("SKILL.md"), "user agents skill\n").expect("write agents");
    fs::write(claude_dir.join("SKILL.md"), "user claude skill\n").expect("write claude");

    let files = write_agent_cli_files("prompt", None, cwd.path().to_str()).expect("write");
    assert!(files.agents_skill_path.is_none());
    assert!(files.claude_skill_path.is_none());
    assert!(files.skill_write_warning.is_none());
    assert_eq!(
      fs::read_to_string(agents_dir.join("SKILL.md")).expect("read agents"),
      "user agents skill\n"
    );
    assert_eq!(
      fs::read_to_string(claude_dir.join("SKILL.md")).expect("read claude"),
      "user claude skill\n"
    );
    cleanup_agent_cli_files(&[files.prompt_path, files.skill_dir]).expect("cleanup");
  }

  // Windows' read-only directory attribute doesn't block creating files inside
  // the directory (unlike Unix's write-permission bit), so this test's simulated
  // permission denial is a no-op there and the skip path never triggers.
  #[cfg(not(windows))]
  #[test]
  fn skips_project_skills_when_cwd_is_not_writable() {
    let cwd = tempfile::TempDir::new().expect("cwd");
    let cwd_path = cwd.path().to_path_buf();
    let mut perms = fs::metadata(&cwd_path).expect("metadata").permissions();
    perms.set_readonly(true);
    fs::set_permissions(&cwd_path, perms).expect("chmod");

    let result = write_agent_cli_files("prompt", None, cwd_path.to_str());

    let mut perms = fs::metadata(&cwd_path).expect("metadata").permissions();
    // Restore write access so the tempdir can clean itself up; this is test
    // teardown, not a security-relevant permission change.
    #[allow(
      clippy::permissions_set_readonly_false,
      reason = "test teardown restores write access so the tempdir can clean up"
    )]
    perms.set_readonly(false);
    let _ = fs::set_permissions(&cwd_path, perms);

    let files = result.expect("agent CLI files should still be written");
    assert!(files.agents_skill_path.is_none());
    assert!(files.claude_skill_path.is_none());
    let warning = files.skill_write_warning.expect("skill write warning");
    assert!(warning.contains("Failed to create .agents/skills/treq"));
    assert!(Path::new(&files.prompt_path).exists());
    assert!(Path::new(&files.skill_dir).is_dir());
    cleanup_agent_cli_files(&[files.prompt_path, files.skill_dir]).expect("cleanup");
  }
}
