//! The command an agent terminal runs on start: the agent CLI with Treq's
//! system prompt, filesystem settings and skills, wrapped so its temp files
//! are removed and Treq hears when it exits.

use super::agent_cli::{write_agent_cli_files, AgentCliFiles};
use super::remote::shell_quote;
use serde_json::{json, Map, Value};
use std::path::Path;

/// Runs in the terminal's shell once the agent subshell ends, so the app can
/// notify "Agent finished". A status of 128 or more means a signal killed the
/// agent, which is how Treq closes or restarts a session, so it stays quiet.
const NOTIFY_ON_AGENT_EXIT: &str = "[ $? -lt 128 ] && treq notify --agent-exited >/dev/null 2>&1";

pub struct AgentPaths<'a> {
  pub workspace_path: Option<&'a str>,
  pub repo_path: &'a str,
  /// Supporting repositories of the main repository; readable, never written.
  pub supporting_repo_paths: &'a [String],
}

/// Append an agent prompt as a positional argument, never as a CLI option.
pub fn append_agent_prompt(command: &str, prompt: &str) -> String {
  format!("{command} -- {}", shell_quote(prompt))
}

/// The single-line system prompt injected into agent terminal sessions.
pub fn system_prompt(paths: &AgentPaths<'_>) -> String {
  let mut parts: Vec<String> = match paths.workspace_path {
    Some(workspace) => vec![
      "You are operating inside a Treq workspace.".into(),
      format!("Your current working directory and direct filesystem scope is {workspace}."),
      format!("The Treq home repository is {}.", paths.repo_path),
      "The home repository and sibling workspaces are outside your direct filesystem scope.".into(),
    ],
    None => vec![
      "You are operating in the Treq home repository.".into(),
      format!(
        "Your current working directory and direct filesystem scope is {}.",
        paths.repo_path
      ),
    ],
  };
  parts.extend(
    [
      "You may freely read and write files within this directory and its descendants.",
      "Do not directly read, write, edit, or delete files outside this directory.",
      "A Treq skill named treq is loaded automatically for this session.",
      "Follow it for the treq CLI, workspaces, sending files to the user, and commits.",
      "You may run treq CLI commands even when they create or manage workspaces outside the current working directory.",
    ]
    .map(String::from),
  );
  if !paths.supporting_repo_paths.is_empty() {
    parts.push(format!(
      "Supporting repositories: {}.",
      paths.supporting_repo_paths.join(", ")
    ));
    parts.extend(
      [
        "You may read files in supporting repositories but must not write to them directly.",
        "To change a supporting repository, create a workspace in it with `treq add -r <repo> <branch>` and start a scoped agent there with `treq agent start -r <repo> <branch> \"<prompt>\"`.",
        "Check its progress with `treq st -r <repo> <branch>`.",
      ]
      .map(String::from),
    );
  }
  parts.join(" ")
}

/// Workspace/home filesystem allow and deny lists for Claude `--settings`.
pub fn claude_filesystem_settings(paths: &AgentPaths<'_>) -> Value {
  let supporting = paths.supporting_repo_paths.iter().cloned();
  match paths.workspace_path {
    Some(workspace) => json!({
      "denyRead": [paths.repo_path],
      "allowRead": std::iter::once(workspace.to_string()).chain(supporting).collect::<Vec<_>>(),
      "allowWrite": [workspace],
    }),
    None => json!({
      "allowRead": std::iter::once(paths.repo_path.to_string()).chain(supporting).collect::<Vec<_>>(),
      "allowWrite": [paths.repo_path],
    }),
  }
}

/// Overlay Treq filesystem restrictions onto `.claude/settings.local.json`, without sandbox.
pub fn merge_claude_local_settings(
  existing: Option<Map<String, Value>>,
  filesystem: Value,
) -> Value {
  let mut settings = existing.unwrap_or_default();
  settings.remove("sandbox");
  settings.insert("filesystem".into(), filesystem);
  Value::Object(settings)
}

pub fn claude_local_settings_path(cwd: &str) -> String {
  format!("{}/.claude/settings.local.json", cwd.trim_end_matches('/'))
}

fn shell_cat(path: &str) -> String {
  format!("$(cat -- {})", shell_quote(path))
}

pub fn cursor_prompt_file_contents(system_prompt: &str, pending_prompt: Option<&str>) -> String {
  match pending_prompt.filter(|p| !p.is_empty()) {
    Some(prompt) => format!("{system_prompt} {prompt}"),
    None => system_prompt.to_string(),
  }
}

pub struct AutoCommand<'a> {
  pub agent: &'a str,
  pub permission_mode: Option<&'a str>,
  pub session_model: Option<&'a str>,
  pub pending_prompt: Option<&'a str>,
  pub treq_bin_dir: Option<&'a str>,
}

/// The PTY auto-command for an agent CLI.
///
/// Long prompt/settings bodies are never inlined. Claude reads files via native
/// flags; Codex and Cursor have no append-from-file flags, so the shell expands
/// `$(cat -- path)` at exec time (the typed command stays short).
pub fn build_auto_command(
  options: &AutoCommand<'_>,
  files: &AgentCliFiles,
) -> Result<String, String> {
  let pending = options.pending_prompt.filter(|p| !p.is_empty());
  let mut cleanup = vec![files.prompt_path.clone()];
  if !files.skill_dir.is_empty() {
    cleanup.push(files.skill_dir.clone());
  }
  cleanup.extend(files.agents_skill_path.clone());
  cleanup.extend(files.claude_skill_path.clone());

  let mut command = match options.agent {
    "codex" => {
      let command = format!(
        "codex -c \"developer_instructions={}\"",
        shell_cat(&files.prompt_path)
      );
      match pending {
        Some(prompt) => append_agent_prompt(&command, prompt),
        None => command,
      }
    }
    "cursor" => {
      let plan = if options.permission_mode == Some("plan") {
        " --plan"
      } else {
        ""
      };
      format!(
        "cursor-agent{plan} -- \"{}\"",
        shell_cat(&files.prompt_path)
      )
    }
    "copilot" => {
      let command = format!(
        "copilot --allow-all-tools -- \"{}\"",
        shell_cat(&files.prompt_path)
      );
      match pending {
        Some(prompt) => append_agent_prompt(&command, prompt),
        None => command,
      }
    }
    _ => {
      let mode = if options.permission_mode == Some("plan") {
        "plan"
      } else {
        "acceptEdits"
      };
      let mut command = format!("claude --permission-mode {mode}");
      if let Some(model) = options.session_model.filter(|m| !m.is_empty()) {
        command.push_str(&format!(" --model={}", shell_quote(model)));
      }
      let settings = files
        .settings_path
        .as_deref()
        .ok_or("Claude auto-command requires a settings file path")?;
      cleanup.push(settings.to_string());
      command.push_str(&format!(" --settings {}", shell_quote(settings)));
      command.push_str(&format!(
        " --append-system-prompt-file {}",
        shell_quote(&files.prompt_path)
      ));
      if let Some(prompt) = pending {
        command.push_str(&format!(" -- {}", shell_quote(prompt)));
      }
      command
    }
  };

  let quoted: Vec<String> = cleanup.iter().map(|p| shell_quote(p)).collect();
  command = format!(
    "( trap \"rm -rf {}\" EXIT; {command} ); {NOTIFY_ON_AGENT_EXIT}",
    quoted.join(" ")
  );
  if let Some(bin) = options.treq_bin_dir.filter(|b| !b.is_empty()) {
    command = format!("export PATH={}:$PATH; {command}", shell_quote(bin));
  }
  Ok(command)
}

pub struct Prepared {
  pub command: String,
  pub file_paths: Vec<String>,
  pub skill_write_warning: Option<String>,
}

/// Writes the agent's prompt, settings and skill files, then builds its command.
pub fn prepare_auto_command(
  paths: &AgentPaths<'_>,
  options: &AutoCommand<'_>,
) -> Result<Prepared, String> {
  let cwd = paths.workspace_path.unwrap_or(paths.repo_path);
  // Writing skills would recreate a deleted workspace as an empty folder.
  if !Path::new(cwd).is_dir() {
    return Err(format!("Workspace folder no longer exists: {cwd}"));
  }
  let system = system_prompt(paths);
  let (prompt, settings) = match options.agent {
    "cursor" => (
      cursor_prompt_file_contents(&system, options.pending_prompt),
      None,
    ),
    "claude" => {
      let existing = std::fs::read_to_string(claude_local_settings_path(cwd))
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|value| match value {
          Value::Object(map) => Some(map),
          _ => None,
        });
      let merged = merge_claude_local_settings(existing, claude_filesystem_settings(paths));
      let json = serde_json::to_string_pretty(&merged).map_err(|e| e.to_string())?;
      (system, Some(json))
    }
    _ => (system, None),
  };

  let mut skipped_project_skills = false;
  let files = match write_agent_cli_files(&prompt, settings.as_deref(), Some(cwd)) {
    Ok(files) => files,
    Err(_) => {
      skipped_project_skills = true;
      write_agent_cli_files(&prompt, settings.as_deref(), None)?
    }
  };
  let file_paths = std::iter::once(files.prompt_path.clone())
    .chain(files.settings_path.clone())
    .chain(Some(files.skill_dir.clone()).filter(|d| !d.is_empty()))
    .chain(files.agents_skill_path.clone())
    .chain(files.claude_skill_path.clone())
    .collect();
  let skill_write_warning = files.skill_write_warning.clone().or_else(|| {
    skipped_project_skills.then(|| {
      "Could not write Treq skills into the workspace. The session still has the bundled copy."
        .to_string()
    })
  });
  let pending_prompt = if options.agent == "cursor" {
    None
  } else {
    options.pending_prompt
  };
  let command = build_auto_command(
    &AutoCommand {
      pending_prompt,
      ..*options
    },
    &files,
  )?;
  Ok(Prepared {
    command,
    file_paths,
    skill_write_warning,
  })
}

/// Strips trailing newlines and ends the command with one carriage return.
pub fn normalize_command(command: &str) -> String {
  format!("{}\r", command.trim_end_matches(['\r', '\n']))
}

#[cfg(test)]
mod tests {
  use super::*;

  fn files() -> AgentCliFiles {
    AgentCliFiles {
      prompt_path: "/tmp/treq-agent-prompt-1.txt".into(),
      settings_path: Some("/tmp/treq-agent-settings-1.json".into()),
      skill_dir: "/tmp/treq-agent-skills-1".into(),
      agents_skill_path: None,
      claude_skill_path: None,
      skill_write_warning: None,
    }
  }

  fn opts(agent: &str) -> AutoCommand<'_> {
    AutoCommand {
      agent,
      permission_mode: None,
      session_model: None,
      pending_prompt: None,
      treq_bin_dir: None,
    }
  }

  const NONE: &[String] = &[];

  #[test]
  fn appends_prompts_as_positional_arguments() {
    assert_eq!(
      append_agent_prompt("codex -c 'instructions=treq'", "---- test failure ----"),
      "codex -c 'instructions=treq' -- '---- test failure ----'"
    );
    assert_eq!(
      append_agent_prompt("codex", "it's $HOME `pwd`!"),
      "codex -- 'it'\\''s $HOME `pwd`!'"
    );
  }

  #[test]
  fn describes_a_workspace_scope() {
    let prompt = system_prompt(&AgentPaths {
      workspace_path: Some("/repos/treq/.treq/workspaces/fix-parser"),
      repo_path: "/repos/treq",
      supporting_repo_paths: NONE,
    });
    assert!(prompt.contains("You are operating inside a Treq workspace."));
    assert!(prompt.contains(
      "Your current working directory and direct filesystem scope is /repos/treq/.treq/workspaces/fix-parser."
    ));
    assert!(prompt.contains("The Treq home repository is /repos/treq."));
    assert!(prompt.contains("A Treq skill named treq is loaded automatically for this session."));
    assert!(!prompt.contains('\n'));
    assert!(!prompt.contains("Supporting repositories"));
  }

  #[test]
  fn describes_the_home_repository_and_supporting_repos() {
    let supporting = vec!["/repos/api".to_string(), "/repos/web".to_string()];
    let prompt = system_prompt(&AgentPaths {
      workspace_path: None,
      repo_path: "/repos/app",
      supporting_repo_paths: &supporting,
    });
    assert!(prompt.contains("You are operating in the Treq home repository."));
    assert!(!prompt.contains("sibling workspaces"));
    assert!(prompt.contains("Supporting repositories: /repos/api, /repos/web."));
    assert!(prompt.contains("treq agent start -r <repo> <branch> \"<prompt>\""));
  }

  #[test]
  fn scopes_claude_filesystem_settings() {
    let supporting = vec!["/repos/api".to_string()];
    assert_eq!(
      claude_filesystem_settings(&AgentPaths {
        workspace_path: Some("/repos/app/.treq/workspaces/feat"),
        repo_path: "/repos/app",
        supporting_repo_paths: &supporting,
      }),
      json!({
        "denyRead": ["/repos/app"],
        "allowRead": ["/repos/app/.treq/workspaces/feat", "/repos/api"],
        "allowWrite": ["/repos/app/.treq/workspaces/feat"],
      })
    );
    assert_eq!(
      claude_filesystem_settings(&AgentPaths {
        workspace_path: None,
        repo_path: "/repos/app",
        supporting_repo_paths: NONE,
      }),
      json!({ "allowRead": ["/repos/app"], "allowWrite": ["/repos/app"] })
    );
  }

  #[test]
  fn merges_local_settings_without_sandbox() {
    let existing = json!({ "extra": true, "sandbox": { "enabled": true } });
    let Value::Object(existing) = existing else {
      unreachable!()
    };
    assert_eq!(
      merge_claude_local_settings(Some(existing), json!({ "allowRead": ["/a"] })),
      json!({ "extra": true, "filesystem": { "allowRead": ["/a"] } })
    );
    assert_eq!(
      merge_claude_local_settings(None, json!({})),
      json!({ "filesystem": {} })
    );
    assert_eq!(
      claude_local_settings_path("/repos/treq/"),
      "/repos/treq/.claude/settings.local.json"
    );
    assert_eq!(
      cursor_prompt_file_contents("sys", Some("do the thing")),
      "sys do the thing"
    );
    assert_eq!(cursor_prompt_file_contents("sys", None), "sys");
  }

  #[test]
  fn points_claude_at_its_settings_and_prompt_files() {
    let files = AgentCliFiles {
      claude_skill_path: Some("/ws/.claude/skills/treq".into()),
      ..files()
    };
    let command = build_auto_command(
      &AutoCommand {
        pending_prompt: Some("fix the bug"),
        ..opts("claude")
      },
      &files,
    )
    .unwrap();
    assert!(command.contains("claude --permission-mode acceptEdits"));
    assert!(command.contains("--settings '/tmp/treq-agent-settings-1.json'"));
    assert!(command.contains("--append-system-prompt-file '/tmp/treq-agent-prompt-1.txt'"));
    assert!(command.contains(
      "trap \"rm -rf '/tmp/treq-agent-prompt-1.txt' '/tmp/treq-agent-skills-1' '/ws/.claude/skills/treq' '/tmp/treq-agent-settings-1.json'\" EXIT"
    ));
    assert!(command.contains("-- 'fix the bug'"));
    let plan = build_auto_command(
      &AutoCommand {
        permission_mode: Some("plan"),
        session_model: Some("opus"),
        ..opts("claude")
      },
      &files,
    )
    .unwrap();
    assert!(plan.contains("--permission-mode plan --model='opus'"));
    assert!(build_auto_command(
      &opts("claude"),
      &AgentCliFiles {
        settings_path: None,
        ..files
      }
    )
    .is_err());
  }

  #[test]
  fn reads_codex_and_cursor_prompts_with_cat() {
    let files = AgentCliFiles {
      settings_path: None,
      agents_skill_path: Some("/ws/.agents/skills/treq".into()),
      ..files()
    };
    let codex = build_auto_command(&opts("codex"), &files).unwrap();
    assert!(codex
      .contains("codex -c \"developer_instructions=$(cat -- '/tmp/treq-agent-prompt-1.txt')\""));
    assert!(codex.contains(
      "trap \"rm -rf '/tmp/treq-agent-prompt-1.txt' '/tmp/treq-agent-skills-1' '/ws/.agents/skills/treq'\" EXIT"
    ));
    let cursor = build_auto_command(
      &AutoCommand {
        permission_mode: Some("plan"),
        ..opts("cursor")
      },
      &files,
    )
    .unwrap();
    assert!(cursor.contains("cursor-agent --plan -- \"$(cat -- '/tmp/treq-agent-prompt-1.txt')\""));
  }

  #[test]
  fn tells_treq_when_the_agent_exits() {
    for agent in ["claude", "codex", "cursor", "copilot"] {
      let command = build_auto_command(
        &AutoCommand {
          treq_bin_dir: Some("/opt/treq/bin"),
          ..opts(agent)
        },
        &files(),
      )
      .unwrap();
      assert!(
        command.starts_with("export PATH='/opt/treq/bin':$PATH; ( trap"),
        "{command}"
      );
      assert!(command.ends_with("); [ $? -lt 128 ] && treq notify --agent-exited >/dev/null 2>&1"));
    }
  }

  #[test]
  fn normalizes_commands_to_one_return() {
    assert_eq!(normalize_command("ls\n\r\n"), "ls\r");
    assert_eq!(normalize_command("ls"), "ls\r");
  }

  #[test]
  fn prepares_a_claude_command_with_the_local_settings_merged() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().to_str().unwrap();
    std::fs::create_dir_all(dir.path().join(".claude")).unwrap();
    std::fs::write(
      dir.path().join(".claude/settings.local.json"),
      r#"{"extra":1,"sandbox":{"enabled":true}}"#,
    )
    .unwrap();
    let prepared = prepare_auto_command(
      &AgentPaths {
        workspace_path: None,
        repo_path: cwd,
        supporting_repo_paths: NONE,
      },
      &AutoCommand {
        pending_prompt: Some("hi"),
        ..opts("claude")
      },
    )
    .unwrap();
    assert!(prepared.command.contains("-- 'hi'"));
    let settings = prepared
      .file_paths
      .iter()
      .find(|p| p.ends_with(".json"))
      .unwrap();
    let written: Value = serde_json::from_str(&std::fs::read_to_string(settings).unwrap()).unwrap();
    assert_eq!(written["extra"], 1);
    assert!(written.get("sandbox").is_none());
    super::super::agent_cli::cleanup_agent_cli_files(&prepared.file_paths).unwrap();
  }

  #[test]
  fn refuses_to_start_in_a_deleted_workspace() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("deleted");
    let error = prepare_auto_command(
      &AgentPaths {
        workspace_path: workspace.to_str(),
        repo_path: dir.path().to_str().unwrap(),
        supporting_repo_paths: NONE,
      },
      &opts("claude"),
    )
    .err()
    .expect("a deleted workspace must not start an agent");
    assert!(error.contains("no longer exists"), "{error}");
    assert!(!workspace.exists());
  }
}
