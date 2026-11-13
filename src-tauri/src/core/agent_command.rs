//! The command an agent terminal runs on start: the agent CLI with Treq's
//! system prompt, filesystem settings and skills, wrapped so its temp files
//! are removed and Treq hears when it exits.

use super::agent_cli::{write_agent_cli_files, write_agent_task_file, AgentCliFiles};
use super::remote::shell_quote;
use serde_json::{json, Map, Value};
use std::path::Path;

/// Runs in the terminal's shell once the agent subshell ends, so the app can
/// notify "Agent finished". A status of 128 or more means a signal killed the
/// agent, which is how Treq closes or restarts a session, so it stays quiet.
const NOTIFY_ON_AGENT_EXIT: &str = "[ $? -lt 128 ] && treq notify --agent-exited >/dev/null 2>&1";

/// The app and repository setting holding instructions added to every agent.
pub const AGENT_INSTRUCTIONS_SETTING: &str = "agent_instructions";

pub struct AgentPaths<'a> {
  pub workspace_path: Option<&'a str>,
  pub repo_path: &'a str,
  /// Supporting repositories of the main repository; readable, never written.
  pub supporting_repo_paths: &'a [String],
  /// The saved project of the window the agent starts in.
  pub project: Option<&'a ProjectContext>,
  /// Instructions from the Application settings, added to every agent.
  pub app_instructions: Option<&'a str>,
  /// Instructions from the repository's settings.
  pub repo_instructions: Option<&'a str>,
}

/// What a project adds to its agents (`core::projects`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProjectContext {
  pub name: String,
  pub prompt: String,
  pub context_paths: Vec<String>,
}

impl From<crate::core::projects::Project> for ProjectContext {
  fn from(project: crate::core::projects::Project) -> Self {
    Self {
      name: project.name,
      prompt: project.prompt,
      context_paths: project.context_paths,
    }
  }
}

impl ProjectContext {
  /// The local paths agents may read (URLs are only named in the prompt).
  fn local_paths(&self) -> impl Iterator<Item = String> + '_ {
    self
      .context_paths
      .iter()
      .filter(|p| !p.contains("://"))
      .cloned()
  }
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
  for (label, text) in [
    ("Application", paths.app_instructions),
    ("Repository", paths.repo_instructions),
  ] {
    let text = one_line(text.unwrap_or_default());
    if !text.is_empty() {
      parts.push(format!("{label} instructions: {text}"));
    }
  }
  if let Some(project) = paths.project {
    parts.push(format!(
      "This work is part of the Treq project \"{}\".",
      project.name
    ));
    parts.push(
      "Run `treq project status` to see every repository and workspace in the project, and `treq project add-repo <owner/name|url|path>` to add (and clone) a repository; never clone it yourself.".into(),
    );
    let prompt = one_line(&project.prompt);
    if !prompt.is_empty() {
      parts.push(format!("Project instructions: {prompt}"));
    }
    if !project.context_paths.is_empty() {
      parts.push(format!(
        "Project context you may read (never write): {}.",
        project.context_paths.join(", ")
      ));
    }
  }
  parts.join(" ")
}

fn one_line(text: &str) -> String {
  text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Workspace/home filesystem allow and deny lists for Claude `--settings`.
pub fn claude_filesystem_settings(paths: &AgentPaths<'_>) -> Value {
  let supporting = paths
    .supporting_repo_paths
    .iter()
    .cloned()
    .chain(paths.project.into_iter().flat_map(|p| p.local_paths()));
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

/// The task as the agent's positional argument, read from its file at exec
/// time: the terminal's line editor takes control bytes in a typed command
/// (Ctrl-C, Ctrl-U, Tab, CR, Esc) as keystrokes, which no quoting survives.
const TASK_ARG: &str = "-- \"${treq_task%.}\"";

/// Reads the task file into `treq_task`; the `.` keeps trailing newlines,
/// which `$(...)` would strip.
fn read_task(path: &str) -> String {
  format!("treq_task=$(cat -- {}; printf .); ", shell_quote(path))
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
/// flags; Codex, Cursor, Devin and Gemini have no append-from-file flags, so the shell expands
/// `$(cat -- path)` at exec time (the typed command stays short).
pub fn build_auto_command(
  options: &AutoCommand<'_>,
  files: &AgentCliFiles,
) -> Result<String, String> {
  let pending = options.pending_prompt.filter(|p| !p.is_empty());
  let with_task = |command: String| match (&files.task_path, pending) {
    (Some(_), Some(_)) => format!("{command} {TASK_ARG}"),
    (None, Some(prompt)) => append_agent_prompt(&command, prompt),
    (_, None) => command,
  };
  let mut cleanup = vec![files.dir.clone()];
  cleanup.extend(files.agents_skill_path.clone());
  cleanup.extend(files.claude_skill_path.clone());

  let mut command = match options.agent {
    "codex" => {
      let command = format!(
        "codex -c \"developer_instructions={}\"",
        shell_cat(&files.prompt_path)
      );
      with_task(command)
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
    "devin" => {
      let plan = if options.permission_mode == Some("plan") {
        " --permission-mode plan"
      } else {
        ""
      };
      format!("devin{plan} -- \"{}\"", shell_cat(&files.prompt_path))
    }
    "gemini" => {
      let mode = if options.permission_mode == Some("plan") {
        "plan"
      } else {
        "auto_edit"
      };
      format!(
        "gemini --approval-mode {mode} -i \"{}\"",
        shell_cat(&files.prompt_path)
      )
    }
    "copilot" => {
      let command = format!(
        "copilot --allow-all-tools -- \"{}\"",
        shell_cat(&files.prompt_path)
      );
      with_task(command)
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
      command.push_str(&format!(" --settings {}", shell_quote(settings)));
      command.push_str(&format!(
        " --append-system-prompt-file {}",
        shell_quote(&files.prompt_path)
      ));
      with_task(command)
    }
  };

  let quoted: Vec<String> = cleanup.iter().map(|p| shell_quote(p)).collect();
  command = format!(
    "( trap \"rm -rf {}\" EXIT; {}{command} ); {NOTIFY_ON_AGENT_EXIT}",
    quoted.join(" "),
    files
      .task_path
      .as_deref()
      .filter(|_| pending.is_some())
      .map(read_task)
      .unwrap_or_default()
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
    // No system-prompt flag: the task rides in the prompt file.
    "cursor" | "devin" | "gemini" => (
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
  let pending_prompt = if matches!(options.agent, "cursor" | "devin" | "gemini") {
    None
  } else {
    options.pending_prompt.filter(|p| !p.is_empty())
  };
  let mut files = files;
  files.task_path = pending_prompt
    .map(|task| write_agent_task_file(&files.dir, task))
    .transpose()?;
  let file_paths = std::iter::once(files.dir.clone())
    .chain(files.agents_skill_path.clone())
    .chain(files.claude_skill_path.clone())
    .collect();
  let skill_write_warning = files.skill_write_warning.clone().or_else(|| {
    skipped_project_skills.then(|| {
      "Could not write Treq skills into the workspace. The session still has the bundled copy."
        .to_string()
    })
  });
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
      dir: "/tmp/treq-agent-1".into(),
      prompt_path: "/tmp/treq-agent-1/prompt.txt".into(),
      settings_path: Some("/tmp/treq-agent-1/settings.json".into()),
      skill_dir: "/tmp/treq-agent-1/skills".into(),
      agents_skill_path: None,
      claude_skill_path: None,
      skill_write_warning: None,
      task_path: None,
    }
  }

  /// The files a launch wrote, looking one level into its temp dir.
  fn launch_files(prepared: &Prepared) -> Vec<String> {
    prepared
      .file_paths
      .iter()
      .flat_map(|p| match std::fs::read_dir(p) {
        Ok(entries) => entries
          .flatten()
          .map(|e| e.path().to_string_lossy().into_owned())
          .collect(),
        Err(_) => vec![p.clone()],
      })
      .collect()
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
      project: None,
      app_instructions: None,
      repo_instructions: None,
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
      project: None,
      app_instructions: None,
      repo_instructions: None,
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
        project: None,
        app_instructions: None,
        repo_instructions: None,
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
        project: None,
        app_instructions: None,
        repo_instructions: None,
      }),
      json!({ "allowRead": ["/repos/app"], "allowWrite": ["/repos/app"] })
    );
  }

  fn project() -> ProjectContext {
    ProjectContext {
      name: "Checkout".into(),
      prompt: "Use pnpm.\nKeep the API and web in sync.".into(),
      context_paths: vec!["/docs/style.md".into(), "https://x.dev/spec".into()],
    }
  }

  #[test]
  fn adds_project_instructions_and_context_on_one_line() {
    let project = project();
    let prompt = system_prompt(&AgentPaths {
      workspace_path: None,
      repo_path: "/repos/app",
      supporting_repo_paths: NONE,
      project: Some(&project),
      app_instructions: None,
      repo_instructions: None,
    });
    assert!(prompt.contains("Treq project \"Checkout\""));
    assert!(prompt.contains("treq project status"));
    assert!(prompt.contains("Project instructions: Use pnpm. Keep the API and web in sync."));
    assert!(prompt
      .contains("Project context you may read (never write): /docs/style.md, https://x.dev/spec."));
    assert!(!prompt.contains('\n'));
  }

  #[test]
  fn leaves_project_out_of_the_prompt_without_a_project() {
    let prompt = system_prompt(&AgentPaths {
      workspace_path: None,
      repo_path: "/repos/app",
      supporting_repo_paths: NONE,
      project: None,
      app_instructions: None,
      repo_instructions: None,
    });
    assert!(!prompt.contains("Treq project"));
    assert!(!prompt.contains("treq project status"));
    assert!(!prompt.contains("Project instructions"));
  }

  #[test]
  fn adds_app_then_repository_instructions_on_one_line() {
    let project = project();
    let prompt = system_prompt(&AgentPaths {
      workspace_path: None,
      repo_path: "/repos/app",
      supporting_repo_paths: NONE,
      project: Some(&project),
      app_instructions: Some("Be terse.\nPrefer TDD."),
      repo_instructions: Some("Use cargo nextest."),
    });
    let app = prompt
      .find("Application instructions: Be terse. Prefer TDD.")
      .unwrap();
    let repo = prompt
      .find("Repository instructions: Use cargo nextest.")
      .unwrap();
    let proj = prompt.find("Project instructions:").unwrap();
    assert!(app < repo && repo < proj);
    assert!(!prompt.contains('\n'));
  }

  #[test]
  fn skips_blank_app_and_repository_instructions() {
    let prompt = system_prompt(&AgentPaths {
      workspace_path: None,
      repo_path: "/repos/app",
      supporting_repo_paths: NONE,
      project: None,
      app_instructions: Some(" \n "),
      repo_instructions: None,
    });
    assert!(!prompt.contains("Application instructions"));
    assert!(!prompt.contains("Repository instructions"));
  }

  #[test]
  fn skips_blank_project_instructions_and_empty_context() {
    let project = ProjectContext {
      name: "Checkout".into(),
      prompt: " \n\t ".into(),
      context_paths: Vec::new(),
    };
    let prompt = system_prompt(&AgentPaths {
      workspace_path: Some("/ws"),
      repo_path: "/repos/app",
      supporting_repo_paths: NONE,
      project: Some(&project),
      app_instructions: None,
      repo_instructions: None,
    });
    assert!(prompt.contains("Treq project \"Checkout\""));
    assert!(!prompt.contains("Project instructions"));
    assert!(!prompt.contains("Project context"));
  }

  #[test]
  fn project_context_comes_from_the_saved_project() {
    let saved = crate::core::projects::Project {
      id: "p".into(),
      name: "Checkout".into(),
      repos: vec!["/r/web".into()],
      created_at: String::new(),
      last_opened_at: String::new(),
      prompt: "Use npm".into(),
      context_paths: vec!["/docs/a.md".into()],
    };
    assert_eq!(
      ProjectContext::from(saved),
      ProjectContext {
        name: "Checkout".into(),
        prompt: "Use npm".into(),
        context_paths: vec!["/docs/a.md".into()],
      }
    );
  }

  #[test]
  fn prepared_claude_command_carries_project_prompt_and_context() {
    let dir = tempfile::TempDir::new().unwrap();
    let repo = dir.path().to_str().unwrap();
    let project = project();
    let prepared = prepare_auto_command(
      &AgentPaths {
        workspace_path: None,
        repo_path: repo,
        supporting_repo_paths: NONE,
        project: Some(&project),
        app_instructions: None,
        repo_instructions: None,
      },
      &opts("claude"),
    )
    .unwrap();
    let texts: Vec<String> = launch_files(&prepared)
      .iter()
      .filter_map(|p| std::fs::read_to_string(p).ok())
      .collect();
    for p in &prepared.file_paths {
      let _ = std::fs::remove_file(p);
      let _ = std::fs::remove_dir_all(p);
    }
    assert!(
      texts
        .iter()
        .any(|t| t.contains("Project instructions: Use pnpm.")),
      "{texts:?}"
    );
    assert!(
      texts
        .iter()
        .any(|t| t.contains("/docs/style.md") && t.contains("allowRead")),
      "settings let claude read the context: {texts:?}"
    );
    assert!(!texts
      .iter()
      .any(|t| t.contains("allowRead") && t.contains("https://x.dev/spec")));
  }

  #[test]
  fn lets_claude_read_local_project_context_only() {
    let project = project();
    assert_eq!(
      claude_filesystem_settings(&AgentPaths {
        workspace_path: None,
        repo_path: "/repos/app",
        supporting_repo_paths: NONE,
        project: Some(&project),
        app_instructions: None,
        repo_instructions: None,
      }),
      json!({ "allowRead": ["/repos/app", "/docs/style.md"], "allowWrite": ["/repos/app"] })
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
    assert!(command.contains("--settings '/tmp/treq-agent-1/settings.json'"));
    assert!(command.contains("--append-system-prompt-file '/tmp/treq-agent-1/prompt.txt'"));
    assert!(command.contains("trap \"rm -rf '/tmp/treq-agent-1' '/ws/.claude/skills/treq'\" EXIT"));
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
      .contains("codex -c \"developer_instructions=$(cat -- '/tmp/treq-agent-1/prompt.txt')\""));
    assert!(codex.contains("trap \"rm -rf '/tmp/treq-agent-1' '/ws/.agents/skills/treq'\" EXIT"));
    let cursor = build_auto_command(
      &AutoCommand {
        permission_mode: Some("plan"),
        ..opts("cursor")
      },
      &files,
    )
    .unwrap();
    assert!(cursor.contains("cursor-agent --plan -- \"$(cat -- '/tmp/treq-agent-1/prompt.txt')\""));
  }

  #[test]
  fn starts_devin_and_gemini_with_the_prompt_file_and_mode() {
    let plan = |agent| {
      build_auto_command(
        &AutoCommand {
          permission_mode: Some("plan"),
          ..opts(agent)
        },
        &files(),
      )
      .unwrap()
    };
    let edit = |agent| build_auto_command(&opts(agent), &files()).unwrap();
    let prompt = "\"$(cat -- '/tmp/treq-agent-1/prompt.txt')\"";
    assert!(plan("devin").contains(&format!("devin --permission-mode plan -- {prompt}")));
    assert!(edit("devin").contains(&format!("devin -- {prompt}")));
    assert!(plan("gemini").contains(&format!("gemini --approval-mode plan -i {prompt}")));
    assert!(edit("gemini").contains(&format!("gemini --approval-mode auto_edit -i {prompt}")));
  }

  #[test]
  fn tells_treq_when_the_agent_exits() {
    for agent in ["claude", "codex", "cursor", "copilot", "devin", "gemini"] {
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
        project: None,
        app_instructions: None,
        repo_instructions: None,
      },
      &AutoCommand {
        pending_prompt: Some("hi"),
        ..opts("claude")
      },
    )
    .unwrap();
    assert!(prepared.command.contains(TASK_ARG));
    assert!(!prepared.command.contains("hi'"));
    let files = launch_files(&prepared);
    let settings = files.iter().find(|p| p.ends_with(".json")).unwrap();
    let written: Value = serde_json::from_str(&std::fs::read_to_string(settings).unwrap()).unwrap();
    assert_eq!(written["extra"], 1);
    assert!(written.get("sandbox").is_none());
    super::super::agent_cli::cleanup_agent_cli_files(&prepared.file_paths).unwrap();
  }

  /// A fresh shell reads typed input in canonical mode, which caps a line
  /// (1024 bytes on macOS, where the Enter is then dropped).
  #[test]
  fn keeps_a_long_task_prompt_out_of_the_typed_command() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().to_str().unwrap();
    let task = format!("Fix the parser.\n{}", "x".repeat(4000));
    for agent in ["claude", "codex", "copilot"] {
      let prepared = prepare_auto_command(
        &AgentPaths {
          workspace_path: None,
          repo_path: cwd,
          supporting_repo_paths: NONE,
          project: None,
          app_instructions: None,
          repo_instructions: None,
        },
        &AutoCommand {
          pending_prompt: Some(&task),
          ..opts(agent)
        },
      )
      .unwrap();
      assert!(
        prepared.command.len() < 1024,
        "{agent}: {}",
        prepared.command
      );
      let files = launch_files(&prepared);
      let task_file = files
        .iter()
        .find(|p| std::fs::read_to_string(p).ok().as_deref() == Some(task.as_str()))
        .unwrap_or_else(|| panic!("{agent}: no file holds the task"));
      assert!(
        prepared.command.contains(&format!(
          "treq_task=$(cat -- {}; printf .); ",
          shell_quote(task_file)
        )),
        "{agent}: {}",
        prepared.command
      );
      assert!(prepared.command.contains(TASK_ARG), "{agent}");
      assert!(prepared
        .command
        .contains(&format!("rm -rf {}", shell_quote(&prepared.file_paths[0]))));
      assert!(prepared.command.contains(&shell_quote(task_file)));
      #[cfg(unix)]
      for (path, expected) in [(task_file, 0o600), (&prepared.file_paths[0], 0o700)] {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, expected, "{agent}: {path} is readable by others");
      }
      super::super::agent_cli::cleanup_agent_cli_files(&prepared.file_paths).unwrap();
    }
  }

  /// The typed `trap` names each temp path, and macOS's `$TMPDIR` is long, so
  /// one launch's temp files share a single dir to keep the line short.
  #[test]
  fn keeps_one_launchs_temp_files_in_a_single_dir() {
    let dir = tempfile::tempdir().unwrap();
    let prepared = prepare_auto_command(
      &AgentPaths {
        workspace_path: None,
        repo_path: dir.path().to_str().unwrap(),
        supporting_repo_paths: NONE,
        project: None,
        app_instructions: None,
        repo_instructions: None,
      },
      &AutoCommand {
        pending_prompt: Some("fix the bug"),
        ..opts("claude")
      },
    )
    .unwrap();
    let temp = std::env::temp_dir();
    let in_temp: Vec<&String> = prepared
      .file_paths
      .iter()
      .filter(|p| Path::new(p).parent() == Some(temp.as_path()))
      .collect();
    super::super::agent_cli::cleanup_agent_cli_files(&prepared.file_paths).unwrap();
    assert_eq!(in_temp.len(), 1, "{:?}", prepared.file_paths);
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
        project: None,
        app_instructions: None,
        repo_instructions: None,
      },
      &opts("claude"),
    )
    .err()
    .expect("a deleted workspace must not start an agent");
    assert!(error.contains("no longer exists"), "{error}");
    assert!(!workspace.exists());
  }

  /// Bytes a shell's line editor inserts as typed. Control bytes (Ctrl-C,
  /// Ctrl-U, Tab, CR, Esc...) are keystrokes to it, so no quoting can type them.
  fn types_as_text(command: &str) -> bool {
    command.chars().all(|c| c == ' ' || c.is_ascii_graphic())
  }

  fn prepare_task(dir: &Path, agent: &str, bin: Option<&str>, prompt: &str) -> Prepared {
    prepare_auto_command(
      &AgentPaths {
        workspace_path: None,
        repo_path: dir.to_str().unwrap(),
        supporting_repo_paths: NONE,
        project: None,
        app_instructions: None,
        repo_instructions: None,
      },
      &AutoCommand {
        pending_prompt: Some(prompt),
        treq_bin_dir: bin,
        ..opts(agent)
      },
    )
    .unwrap()
  }

  proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(64))]

    /// The task never rides in the typed line, so nothing in it reaches
    /// the line editor; the task file holds it byte for byte.
    #[test]
    fn typed_command_never_carries_the_task(prompt in "\\PC*|[\\x00-\\x1f\\x7f'`$()\"!\\\\]{1,40}") {
      let dir = tempfile::tempdir().unwrap();
      for agent in ["claude", "codex", "copilot"] {
        let prepared = prepare_task(dir.path(), agent, None, &prompt);
        let task = prepared.file_paths.iter().find(|p| p.contains("treq-agent-task-")).cloned();
        let written = task.as_deref().map(std::fs::read_to_string);
        super::super::agent_cli::cleanup_agent_cli_files(&prepared.file_paths).unwrap();
        proptest::prop_assert!(types_as_text(&prepared.command), "{:?}", prepared.command);
        if prompt.is_empty() {
          proptest::prop_assert!(task.is_none());
        } else {
          proptest::prop_assert_eq!(written.unwrap().unwrap(), prompt.clone());
        }
      }
    }
  }

  /// Types the agent command into an interactive shell on a real PTY, as
  /// the terminal does, and returns the task the fake agent received.
  #[cfg(unix)]
  fn start_in_pty(shell: &str, prompt: &str) -> Option<String> {
    use portable_pty::{native_pty_system, CommandBuilder, PtySize};
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};

    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let out = dir.path().join("received");
    let agent = bin.join("claude");
    std::fs::write(
      &agent,
      "#!/bin/sh\nwhile [ \"$1\" != -- ]; do shift; done\nprintf %s \"$2\" > \"$TREQ_TEST_OUT\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&agent, std::fs::Permissions::from_mode(0o755)).unwrap();
    let prepared = prepare_task(dir.path(), "claude", bin.to_str(), prompt);

    let pair = native_pty_system()
      .openpty(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
      })
      .unwrap();
    let mut cmd = CommandBuilder::new(shell);
    match shell {
      "zsh" => cmd.args(["-f", "-i"]),
      _ => cmd.args(["--norc", "--noprofile", "-i"]),
    };
    cmd.env("TREQ_TEST_OUT", &out);
    cmd.env("PATH", std::env::var("PATH").unwrap_or_default());
    let mut child = pair.slave.spawn_command(cmd).unwrap();
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    std::thread::spawn(move || std::io::copy(&mut reader, &mut std::io::sink()));
    let mut writer = pair.master.take_writer().unwrap();
    std::thread::sleep(Duration::from_millis(100));
    writer
      .write_all(normalize_command(&prepared.command).as_bytes())
      .unwrap();

    let deadline = Instant::now() + Duration::from_secs(10);
    while !out.exists() && Instant::now() < deadline {
      std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(Duration::from_millis(50));
    child.kill().ok();
    std::fs::read(&out)
      .ok()
      .map(|b| String::from_utf8(b).unwrap())
  }

  #[cfg(unix)]
  fn shells() -> Vec<&'static str> {
    ["bash", "zsh"]
      .into_iter()
      .filter(|s| {
        std::process::Command::new(s)
          .arg("-c")
          .arg(":")
          .status()
          .is_ok_and(|s| s.success())
      })
      .collect()
  }

  #[cfg(unix)]
  #[test]
  fn agent_receives_the_task_byte_for_byte_from_a_typed_start() {
    let long = (0..100)
      .map(|i| format!("line {i}: it's `pwd` $HOME (a \"b\" ! \\ \t tab"))
      .collect::<Vec<_>>()
      .join("\n");
    assert!(long.len() > 4096);
    let mut prompts: Vec<String> = [
      "Run treq st to read this workspace's description",
      "back`tick",
      "$HOME and ${PATH}",
      "open ( paren",
      "close ) paren",
      "a \"double\" quote",
      "bang! !! !$",
      "multi\nline\nprompt",
      "trailing newlines\n\n",
      "unicode: é 中文 🎉 \u{200b}",
      "crlf\r\nline",
    ]
    .map(String::from)
    .to_vec();
    // Every control byte but NUL, which no argv can carry.
    prompts.extend(
      (1u8..32)
        .chain([127])
        .map(|b| format!("it's {} ok", b as char)),
    );
    prompts.push(long);
    for shell in shells() {
      for prompt in &prompts {
        assert_eq!(
          start_in_pty(shell, prompt).as_deref(),
          Some(prompt.as_str()),
          "{shell}: {prompt:?}"
        );
      }
    }
  }

  #[cfg(unix)]
  #[test]
  fn starting_twice_on_a_workspace_starts_both() {
    for shell in shells() {
      for _ in 0..2 {
        let prompt = "Run treq st to read this workspace's description";
        assert_eq!(
          start_in_pty(shell, prompt).as_deref(),
          Some(prompt),
          "{shell}"
        );
      }
    }
  }
}
