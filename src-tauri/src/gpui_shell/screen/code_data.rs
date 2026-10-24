//! What the workspace Code tab (`ShowWorkspace`'s Overview) reads from and
//! writes to the backend, and the rules its React components apply to it:
//! the task input's agent, branch names and mentions, the stack, and the
//! commit history column.

use std::{
  collections::{HashMap, HashSet},
  sync::LazyLock,
};

use regex::{Regex, RegexSet};

use super::{settings_data::SettingsHost, workspace_data::WorkspaceHost};
use crate::{
  core::{
    self,
    devcontainer::{self, DevcontainerStatus},
    feature_preview::{self, PreviewFeature},
    MaybeEmptyParam, WorkspaceEntry,
  },
  gpui_shell::{
    native::Backend,
    remote_repo::{unsupported, UNSUPPORTED},
    time,
  },
  jj::{JjLogCommit, JjLogResult},
  local_db::Workspace,
};

/// The agents treq launches (`lib/agents.ts`'s `AGENTS`), in picker order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Agent {
  #[default]
  Claude,
  Codex,
  Cursor,
  Copilot,
}

impl Agent {
  pub const ALL: [Agent; 4] = [Agent::Claude, Agent::Codex, Agent::Cursor, Agent::Copilot];

  pub fn id(self) -> &'static str {
    match self {
      Agent::Claude => "claude",
      Agent::Codex => "codex",
      Agent::Cursor => "cursor",
      Agent::Copilot => "copilot",
    }
  }

  pub fn label(self) -> &'static str {
    match self {
      Agent::Claude => "Claude",
      Agent::Codex => "Codex",
      Agent::Cursor => "Cursor",
      Agent::Copilot => "Copilot",
    }
  }

  /// The command-line tool that runs the agent.
  pub fn binary(self) -> &'static str {
    match self {
      Agent::Claude => "claude",
      Agent::Codex => "codex",
      Agent::Cursor => "cursor-agent",
      Agent::Copilot => "copilot",
    }
  }

  /// Offers "Plan" next to Edit.
  pub fn has_plan_mode(self) -> bool {
    matches!(self, Agent::Claude | Agent::Cursor)
  }

  /// `toAgentKind`: a stored setting, dropping anything unrecognized.
  pub fn parse(value: Option<&str>) -> Option<Self> {
    Self::ALL.into_iter().find(|a| Some(a.id()) == value)
  }
}

/// The repository setting for the home view's "Run in a new workspace".
pub const NEW_WORKSPACE_SETTING: &str = "home_task_new_workspace";
/// Where new terminals and agents run (`ENVIRONMENT_SETTING`).
pub const ENVIRONMENT_SETTING: &str = "environment";

/// The files and README the Code tab lists.
#[derive(Debug, Clone, Default)]
pub struct Overview {
  pub entries: Vec<WorkspaceEntry>,
  pub readme: Option<String>,
  /// The repository is remote, which has no file tree yet.
  pub unavailable: bool,
}

/// What the Code tab's file list says for a remote repository.
pub const REMOTE_FILE_TREE_REASON: &str = "Browsing the file tree is not available for remote repositories yet. Use Go to file to open a file.";

impl Overview {
  /// `ShowWorkspace`'s overview read: a remote repository's refusal shows
  /// as an empty, unavailable tree; any other failure as an empty one.
  pub fn or_unavailable(read: Result<Overview, String>) -> Overview {
    read.unwrap_or_else(|err| {
      let unavailable = err.starts_with(UNSUPPORTED);
      if !unavailable {
        log::warn!("Failed to load workspace overview: {err}");
      }
      Overview {
        unavailable,
        ..Overview::default()
      }
    })
  }
}

/// The task input's settings for a repository.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TaskSettings {
  /// The repository's agent, else the app's (`fromRepo` tells which).
  pub agent: Agent,
  pub from_repo: bool,
  /// `home_task_new_workspace` (on unless "false").
  pub new_workspace: bool,
  /// The editor font size the file list uses (`terminal_font_size`).
  pub font_size: f32,
  /// `None` while the devcontainers preview is off.
  pub environment: Option<Environment>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Environment {
  pub devcontainer: bool,
  pub status: Option<DevcontainerStatus>,
}

/// What the Code tab asks of the backend; faked in tests. Everything but
/// `notify` blocks, so it is called off the main thread.
pub trait CodeHost: WorkspaceHost + SettingsHost {
  fn overview(&self, repo_path: &str, workspace_id: Option<i64>) -> Result<Overview, String>;
  fn commits(
    &self,
    repo_path: &str,
    workspace_id: Option<i64>,
    include_target: bool,
    limit: Option<usize>,
  ) -> Result<JjLogResult, String>;
  /// Every workspace's current record (`listWorkspaceStatuses`).
  fn workspaces(&self, repo_path: &str) -> Result<Vec<Workspace>, String>;
  fn set_submodule_synced(&self, repo_path: &str, path: &str, enabled: bool) -> Result<(), String>;
  fn set_description(
    &self,
    repo_path: &str,
    workspace_id: i64,
    description: &str,
  ) -> Result<(), String>;
  fn set_title(&self, repo_path: &str, workspace_id: i64, title: &str) -> Result<(), String>;
  /// Builds the file index the first time, then searches it.
  fn search_files(
    &self,
    repo_path: &str,
    workspace_id: Option<i64>,
    workspace_path: &str,
    query: &str,
    limit: usize,
  ) -> Result<Vec<String>, String>;
  fn devcontainers_enabled(&self) -> bool;
  fn devcontainer_status(&self, repo_path: &str) -> DevcontainerStatus;
  fn devcontainer_up(&self, repo_path: &str) -> Result<DevcontainerStatus, String>;
  /// Taken branch names: workspaces' and local branches'.
  fn branch_names(&self, repo_path: &str) -> Result<HashSet<String>, String>;
  /// `create_workspace`, with the repository's copy files and setup script.
  fn create_workspace(&self, repo_path: &str, branch: &str, title: &str) -> Result<i64, String>;
  fn create_session(
    &self,
    repo_path: &str,
    workspace_id: Option<i64>,
    name: &str,
  ) -> Result<i64, String>;
  /// The native "Attach Files" dialog; `None` when cancelled.
  fn pick_files(&self) -> Option<Vec<String>>;
  /// The workspace setup script's status; `None` when it cannot be read.
  fn setup(&self, repo_path: &str, workspace_id: i64) -> Option<crate::core::SetupScriptStatus>;
  /// The output of the setup script's run `run_id`.
  fn setup_logs(
    &self,
    repo_path: &str,
    run_id: i64,
  ) -> Result<Vec<super::github_data::LogLine>, String>;
  /// Runs the workspace setup script to the end.
  fn run_setup(
    &self,
    target: &crate::core::running_checks::ChecksTarget,
  ) -> Result<Vec<crate::core::checks::JobResult>, String>;
}

impl CodeHost for Backend {
  fn overview(&self, repo_path: &str, workspace_id: Option<i64>) -> Result<Overview, String> {
    if self.remote_containing(repo_path).is_some() {
      return Err(unsupported("Browsing the file tree"));
    }
    Ok(Overview {
      entries: core::list_workspace_files(repo_path, workspace_id)?,
      readme: core::get_workspace_readme(repo_path, workspace_id)?,
      unavailable: false,
    })
  }

  fn commits(
    &self,
    repo_path: &str,
    workspace_id: Option<i64>,
    include_target: bool,
    limit: Option<usize>,
  ) -> Result<JjLogResult, String> {
    if let Some(remote) = self.remote_containing(repo_path) {
      return remote.commits(workspace_id);
    }
    core::list_commits(repo_path, workspace_id, include_target, None, limit)
  }

  fn workspaces(&self, repo_path: &str) -> Result<Vec<Workspace>, String> {
    if let Some(remote) = self.remote_containing(repo_path) {
      return remote.workspaces();
    }
    Ok(
      core::list_workspace_statuses(repo_path)?
        .into_iter()
        .map(|status| status.current)
        .collect(),
    )
  }

  fn set_submodule_synced(&self, repo_path: &str, path: &str, enabled: bool) -> Result<(), String> {
    if self.remote_containing(repo_path).is_some() {
      return Err(unsupported("Syncing a git submodule"));
    }
    core::set_submodule_synced(repo_path, path, enabled).map(|_| ())
  }

  fn set_description(
    &self,
    repo_path: &str,
    workspace_id: i64,
    description: &str,
  ) -> Result<(), String> {
    if let Some(remote) = self.remote_containing(repo_path) {
      return remote
        .update_workspace(workspace_id, None, None, Some(description))
        .map(drop);
    }
    let description = if description.is_empty() {
      MaybeEmptyParam::EmptyValue
    } else {
      MaybeEmptyParam::Some(description.to_string())
    };
    core::update_workspace(
      repo_path,
      workspace_id,
      MaybeEmptyParam::Omitted,
      description,
    )
    .map(|_| ())
  }
  fn set_title(&self, repo_path: &str, workspace_id: i64, title: &str) -> Result<(), String> {
    if let Some(remote) = self.remote_containing(repo_path) {
      return remote
        .update_workspace(workspace_id, None, Some(title), None)
        .map(drop);
    }
    core::update_workspace_with_title(
      repo_path,
      workspace_id,
      MaybeEmptyParam::Omitted,
      MaybeEmptyParam::Some(title.to_string()),
      MaybeEmptyParam::Omitted,
    )
    .map(|_| ())
  }

  fn search_files(
    &self,
    repo_path: &str,
    workspace_id: Option<i64>,
    workspace_path: &str,
    query: &str,
    limit: usize,
  ) -> Result<Vec<String>, String> {
    // A remote repository searches on the host; it has no local index.
    if let Some(remote) = self.remote_containing(repo_path) {
      return remote.search_files(workspace_id, query, limit);
    }
    crate::commands::workspace::ensure_workspace_indexed(
      repo_path.to_string(),
      workspace_id,
      workspace_path.to_string(),
    )?;
    core::search_workspace_files(repo_path, workspace_id, query, limit)
  }

  fn devcontainers_enabled(&self) -> bool {
    self.with_db(|db| feature_preview::is_enabled(db, PreviewFeature::Devcontainers))
  }

  fn devcontainer_status(&self, repo_path: &str) -> DevcontainerStatus {
    devcontainer::status(repo_path)
  }

  fn devcontainer_up(&self, repo_path: &str) -> Result<DevcontainerStatus, String> {
    let claude_code = self.with_db(|db| {
      db.get_repo_setting(repo_path, devcontainer::CLAUDE_CODE_SETTING)
        .ok()
        .flatten()
        .is_none_or(|value| value != "false")
    });
    devcontainer::up(repo_path, claude_code)
  }

  fn branch_names(&self, repo_path: &str) -> Result<HashSet<String>, String> {
    let (workspaces, branches) = match self.remote_containing(repo_path) {
      Some(remote) => (remote.workspaces()?, remote.branches()?),
      None => (
        core::list_workspaces(repo_path)?,
        core::list_repo_branches(repo_path)?,
      ),
    };
    Ok(
      workspaces
        .into_iter()
        .map(|w| w.branch_name)
        .chain(branches.into_iter().map(|b| b.name))
        .collect(),
    )
  }

  fn create_workspace(&self, repo_path: &str, branch: &str, title: &str) -> Result<i64, String> {
    if let Some(remote) = self.remote_containing(repo_path) {
      // Copy files and the setup script are local repository settings,
      // which a remote repository has none of.
      let metadata = core::WorkspaceMetadata {
        title: Some(title.trim().to_string()).filter(|t| !t.is_empty()),
        ..Default::default()
      };
      let metadata = serde_json::to_string(&metadata).map_err(|e| e.to_string())?;
      return remote.create_workspace(branch, None, Some(metadata));
    }
    let copy_files = self.with_db(|db| {
      let raw = db
        .get_repo_setting(repo_path, "included_copy_files")
        .ok()
        .flatten();
      core::parse_included_copy_files(raw.as_deref())
    });
    let metadata = core::WorkspaceMetadata {
      title: Some(title.trim().to_string()).filter(|t| !t.is_empty()),
      ..Default::default()
    };
    let workspace = core::create_new_workspace(repo_path, branch, None, metadata, copy_files)?;
    crate::commands::workspace::spawn_setup_script_if_configured(&workspace);
    Ok(workspace.id)
  }

  fn create_session(
    &self,
    repo_path: &str,
    workspace_id: Option<i64>,
    name: &str,
  ) -> Result<i64, String> {
    let scope = self.state_scope(repo_path);
    self.with_db(|db| core::sessions::create_session(db, &scope, workspace_id, name.into()))
  }

  fn pick_files(&self) -> Option<Vec<String>> {
    let (tx, rx) = std::sync::mpsc::channel();
    let app = self.app().clone();
    // The dialog opens from the main thread; this waits off it.
    let _ = self.app().run_on_main_thread(move || {
      use tauri_plugin_dialog::DialogExt;
      app
        .dialog()
        .file()
        .set_title("Attach Files")
        .pick_files(move |files| {
          let _ = tx.send(files);
        });
    });
    let files = rx.recv().ok()??;
    Some(
      files
        .into_iter()
        .filter_map(|f| f.into_path().ok())
        .map(|p| p.to_string_lossy().into_owned())
        .collect(),
    )
  }

  fn setup(&self, repo_path: &str, workspace_id: i64) -> Option<crate::core::SetupScriptStatus> {
    core::get_setup_script_status_sync(repo_path, workspace_id).ok()
  }

  fn setup_logs(
    &self,
    repo_path: &str,
    run_id: i64,
  ) -> Result<Vec<super::github_data::LogLine>, String> {
    super::github_data::GithubHost::run_logs(self, repo_path, run_id, core::SETUP_SCRIPT_JOB_ID)
  }

  fn run_setup(
    &self,
    target: &crate::core::running_checks::ChecksTarget,
  ) -> Result<Vec<crate::core::checks::JobResult>, String> {
    core::running_checks::run(target, &core::running_checks::CheckRun::Setup)
  }
}

/// Reads the task input's settings (`TaskInput`'s `agentSettings` and
/// `newWorkspaceSetting`, `useRepoEnvironment`).
pub fn task_settings(host: &dyn CodeHost, repo_path: &str) -> TaskSettings {
  let keys = |keys: &[&str]| keys.iter().map(|k| k.to_string()).collect::<Vec<_>>();
  let repo = host
    .repo_settings(
      repo_path,
      &keys(&["default_agent", NEW_WORKSPACE_SETTING, ENVIRONMENT_SETTING]),
    )
    .unwrap_or_default();
  let app = host
    .settings(&keys(&["default_agent", "terminal_font_size"]))
    .unwrap_or_default();
  let get =
    |values: &HashMap<String, Option<String>>, key: &str| values.get(key).cloned().flatten();
  let repo_agent = Agent::parse(get(&repo, "default_agent").as_deref());
  let environment = host.devcontainers_enabled().then(|| Environment {
    devcontainer: get(&repo, ENVIRONMENT_SETTING).as_deref() == Some("devcontainer"),
    status: Some(host.devcontainer_status(repo_path)),
  });
  TaskSettings {
    agent: repo_agent
      .or_else(|| Agent::parse(get(&app, "default_agent").as_deref()))
      .unwrap_or_default(),
    from_repo: repo_agent.is_some(),
    new_workspace: get(&repo, NEW_WORKSPACE_SETTING).as_deref() != Some("false"),
    font_size: get(&app, "terminal_font_size")
      .and_then(|v| v.parse().ok())
      .unwrap_or(14.),
    environment,
  }
}

pub use crate::core::new_workspaces::apply_branch_name_pattern;

/// `taskBranchSlug`: lowercase ASCII words joined by `-`, cut on a word.
fn task_branch_slug(prompt: &str) -> String {
  const MAX: usize = 48;
  let slug = Regex::new("[a-z0-9]+")
    .expect("regex")
    .find_iter(&prompt.to_lowercase())
    .map(|m| m.as_str().to_string())
    .collect::<Vec<_>>()
    .join("-");
  if slug.len() <= MAX {
    return slug;
  }
  let cut = &slug[..MAX];
  match cut.rfind('-') {
    Some(dash) if dash > 0 => cut[..dash].to_string(),
    _ => cut.to_string(),
  }
}

/// `taskBranchName`: a branch for the task no workspace or branch uses yet.
pub fn task_branch_name(prompt: &str, taken: &HashSet<String>, pattern: &str) -> String {
  let slug = task_branch_slug(prompt);
  let base = apply_branch_name_pattern(pattern, if slug.is_empty() { "task" } else { &slug });
  let mut name = base.clone();
  let mut n = 2;
  while taken.contains(&name) {
    name = format!("{base}-{n}");
    n += 1;
  }
  name
}

pub use crate::core::agent_launch::session_name;

/// The `@` being typed before `cursor` (a byte offset): its offset and the
/// query after it, if it starts the text or follows whitespace.
pub fn mention_at(text: &str, cursor: usize) -> Option<(usize, String)> {
  let before = text.get(..cursor)?;
  for (i, ch) in before.char_indices().rev() {
    if ch == '@' {
      let starts_word = before[..i]
        .chars()
        .next_back()
        .is_none_or(char::is_whitespace);
      return starts_word.then(|| (i, before[i + 1..].to_string()));
    }
    if ch.is_whitespace() {
      return None;
    }
  }
  None
}

/// `insertAtCursor`: `insertion` at `cursor`, after a space unless the text
/// before already ends in one. Returns the text and the new cursor.
pub fn insert_at(text: &str, cursor: usize, insertion: &str) -> (String, usize) {
  let cursor = cursor.min(text.len());
  let (before, after) = text.split_at(cursor);
  let space = if before.is_empty() || before.ends_with(char::is_whitespace) {
    ""
  } else {
    " "
  };
  let inserted = format!("{space}{insertion}");
  (
    format!("{before}{inserted}{after}"),
    before.len() + inserted.len(),
  )
}

/// `looksLikeMarkdown`: block syntax at a line start, or inline emphasis,
/// code or a link.
pub fn looks_like_markdown(text: &str) -> bool {
  static PATTERNS: LazyLock<RegexSet> = LazyLock::new(|| {
    RegexSet::new([
      r"(?m)^#{1,6}\s",
      r"(?m)^\s*[-*+]\s",
      r"(?m)^\s*\d+\.\s",
      r"(?m)^>\s",
      r"(?m)^```",
      r"(?m)^\|.*\|\s*$",
      r"\*\*[^*\n]+\*\*",
      r"`[^`\n]+`",
      r"\[[^\]\n]+\]\([^)\s]+\)",
    ])
    .expect("markdown regexes")
  });
  PATTERNS.is_match(text)
}

/// GitHub emoji shortcodes (`:robot:`) as emoji, outside code. Unknown
/// shortcodes stay as typed.
pub fn emojify(text: &str) -> String {
  static SHORTCODE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r":([a-z0-9_+-]+):").expect("regex"));
  let mut fenced = false;
  text
    .split_inclusive('\n')
    .map(|line| {
      let trimmed = line.trim_start();
      if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
        fenced = !fenced;
        return line.to_string();
      }
      if fenced {
        return line.to_string();
      }
      // Odd pieces between backticks are inline code.
      line
        .split('`')
        .enumerate()
        .map(|(i, piece)| {
          if i % 2 == 1 {
            return piece.to_string();
          }
          SHORTCODE
            .replace_all(piece, |c: &regex::Captures| {
              emojis::get_by_shortcode(&c[1])
                .map_or_else(|| c[0].to_string(), |e| e.as_str().to_string())
            })
            .into_owned()
        })
        .collect::<Vec<_>>()
        .join("`")
    })
    .collect()
}

/// `getWorkspaceDisplayTitle`.
pub fn display_title(workspace: &Workspace) -> String {
  if !workspace.title.is_empty() {
    return workspace.title.clone();
  }
  metadata_field(workspace, "title").unwrap_or_else(|| workspace.branch_name.clone())
}

fn metadata_field(workspace: &Workspace, key: &str) -> Option<String> {
  workspace
    .metadata
    .as_deref()
    .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
    .and_then(|m| m[key].as_str().map(str::to_string))
    .filter(|v| !v.is_empty())
}

/// `workspaceDescription`: the description, else the metadata's.
pub fn description(workspace: &Workspace) -> Option<String> {
  workspace
    .description
    .clone()
    .filter(|d| !d.is_empty())
    .or_else(|| metadata_field(workspace, "description"))
}

/// `getWorkspaceStack` over workspaces not on `default_branch`: the chain
/// through `current_id`, tip first, flagged where current. `None` when the
/// workspace stands alone.
pub fn workspace_stack(
  workspaces: &[Workspace],
  current_id: i64,
  default_branch: &str,
) -> Option<Vec<(Workspace, bool)>> {
  let workspaces: Vec<&Workspace> = workspaces
    .iter()
    .filter(|w| w.branch_name != default_branch)
    .collect();
  let current = *workspaces.iter().find(|w| w.id == current_id)?;

  let mut children: HashMap<&str, Vec<&Workspace>> = HashMap::new();
  for ws in &workspaces {
    if let Some(target) = ws.target_branch.as_deref() {
      children.entry(target).or_default().push(ws);
    }
  }
  let mut descendants = Vec::new();
  let mut stack = vec![current.branch_name.as_str()];
  let mut visited: HashSet<&str> = HashSet::from([current.branch_name.as_str()]);
  while let Some(branch) = stack.pop() {
    for child in children.get(branch).into_iter().flatten() {
      if visited.insert(child.branch_name.as_str()) {
        descendants.push(*child);
        stack.push(child.branch_name.as_str());
      }
    }
  }

  let by_branch: HashMap<&str, &Workspace> = workspaces
    .iter()
    .map(|w| (w.branch_name.as_str(), *w))
    .collect();
  let mut ancestors = Vec::new();
  let mut visited: HashSet<&str> = HashSet::from([current.branch_name.as_str()]);
  let mut branch = current.branch_name.as_str();
  while let Some(parent) = by_branch
    .get(branch)
    .and_then(|ws| ws.target_branch.as_deref())
    .and_then(|target| by_branch.get(target))
  {
    if !visited.insert(parent.branch_name.as_str()) {
      break;
    }
    ancestors.push(*parent);
    branch = parent.branch_name.as_str();
  }
  if descendants.is_empty() && ancestors.is_empty() {
    return None;
  }
  Some(
    ancestors
      .into_iter()
      .rev()
      .chain(std::iter::once(current))
      .chain(descendants)
      .rev()
      .map(|w| (w.clone(), w.id == current_id))
      .collect(),
  )
}

/// `sumWorkspaceDiffStats` over a workspace's own commits.
pub fn diff_stats(commits: &[JjLogCommit]) -> (u32, u32) {
  commits
    .iter()
    .filter(|c| !c.on_target_only)
    .fold((0, 0), |(i, d), c| (i + c.insertions, d + c.deletions))
}

/// `LinearCommitHistory`'s split of a log: the workspace's own commits
/// (without the working copy, or the empty commit on top of it) and the
/// target branch's.
pub fn history_commits(log: &JjLogResult) -> (Vec<JjLogCommit>, Vec<JjLogCommit>) {
  let (target, own): (Vec<_>, Vec<_>) = log.commits.iter().cloned().partition(|c| c.on_target_only);
  let own: Vec<_> = own.into_iter().filter(|c| !c.is_working_copy).collect();
  let own = match own.as_slice() {
    [first, second, ..]
      if first.description == "(no description)"
        && first.bookmarks.is_empty()
        && !first.is_immutable
        && first.parent_ids.contains(&second.commit_id) =>
    {
      own[1..].to_vec()
    }
    _ => own,
  };
  (own, target)
}

/// `groupCommitsByDay`: runs of commits on the same local day, labelled.
pub fn group_by_day(commits: &[JjLogCommit]) -> Vec<(String, Vec<JjLogCommit>)> {
  let mut groups: Vec<(Option<chrono::NaiveDate>, String, Vec<JjLogCommit>)> = Vec::new();
  for commit in commits {
    let day = time::day(&commit.timestamp);
    match groups.last_mut() {
      Some((last, _, list)) if *last == day => list.push(commit.clone()),
      _ => groups.push((
        day,
        time::day_label(&commit.timestamp),
        vec![commit.clone()],
      )),
    }
  }
  groups
    .into_iter()
    .map(|(_, label, list)| (label, list))
    .collect()
}

/// `getCommitHeadline`.
pub fn headline(commit: &JjLogCommit, tentative: bool) -> String {
  let first = commit.description.lines().next().unwrap_or("").trim();
  if !first.is_empty() && first != "(no description)" {
    first.to_string()
  } else if tentative {
    "Working copy".into()
  } else {
    "Untitled commit".into()
  }
}

/// `conflictCount`: unique, trimmed conflicted paths.
pub fn conflict_count(paths: &[String]) -> usize {
  paths
    .iter()
    .map(|p| p.trim())
    .filter(|p| !p.is_empty())
    .collect::<HashSet<_>>()
    .len()
}

#[cfg(test)]
mod tests {
  use super::*;

  fn workspace(id: i64, branch: &str, target: Option<&str>) -> Workspace {
    Workspace {
      id,
      repo_path: "/repo".into(),
      workspace_name: branch.into(),
      workspace_path: branch.into(),
      branch_name: branch.into(),
      created_at: "2026-10-19T10:00:00Z".into(),
      refreshed_at: None,
      metadata: None,
      target_branch: target.map(str::to_string),
      title: String::new(),
      description: None,
      moved_files: None,
      not_on_remote: false,
      sparse_patterns: None,
      archived: false,
    }
  }

  fn commit(id: &str, description: &str) -> JjLogCommit {
    JjLogCommit {
      commit_id: id.into(),
      short_id: id.into(),
      change_id: format!("c{id}"),
      description: description.into(),
      author_name: "Zn".into(),
      timestamp: "2026-10-19T10:00:00Z".into(),
      parent_ids: vec![],
      is_working_copy: false,
      workspace_label: None,
      bookmarks: vec![],
      is_immutable: false,
      insertions: 1,
      deletions: 2,
      on_target_only: false,
      has_conflicts: false,
    }
  }

  #[test]
  fn branch_names_follow_the_pattern_and_skip_taken_ones() {
    let taken = HashSet::from(["treq/fix-the-login-bug".to_string()]);
    assert_eq!(
      task_branch_name("Fix the login bug!", &HashSet::new(), "treq/{name}"),
      "treq/fix-the-login-bug"
    );
    assert_eq!(
      task_branch_name("Fix the login bug!", &taken, "treq/{name}"),
      "treq/fix-the-login-bug-2"
    );
    assert_eq!(task_branch_name("???", &HashSet::new(), "{name}"), "task");
    let long = "word ".repeat(20);
    let name = task_branch_name(&long, &HashSet::new(), "{name}");
    assert!(name.len() <= 48 && !name.ends_with('-'), "{name}");
  }

  #[test]
  fn branch_patterns_drop_trailing_single_letters() {
    assert_eq!(
      apply_branch_name_pattern("{name}", "feat thing r"),
      "feat-thing"
    );
    assert_eq!(
      apply_branch_name_pattern("me/{name}/", "Add API"),
      "me/add-api"
    );
    assert_eq!(apply_branch_name_pattern("{name}", "!!!"), "unnamed");
  }

  #[test]
  fn session_names_cut_long_prompts() {
    assert_eq!(session_name("short"), "short");
    let long = "x".repeat(60);
    assert_eq!(session_name(&long), format!("{}...", "x".repeat(47)));
  }

  #[test]
  fn mentions_start_a_word() {
    assert_eq!(mention_at("@src", 4), Some((0, "src".into())));
    assert_eq!(mention_at("look at @ma", 11), Some((8, "ma".into())));
    assert_eq!(mention_at("a@b", 3), None);
    assert_eq!(mention_at("@a b", 4), None);
    assert_eq!(mention_at("@", 1), Some((0, String::new())));
  }

  #[test]
  fn inserts_after_a_space() {
    assert_eq!(insert_at("fix", 3, "@a.rs "), ("fix @a.rs ".into(), 10));
    assert_eq!(insert_at("fix ", 4, "@a.rs "), ("fix @a.rs ".into(), 10));
    assert_eq!(insert_at("", 0, "x"), ("x".into(), 1));
  }

  #[test]
  fn emojifies_shortcodes_outside_code() {
    assert_eq!(emojify(":robot: done :+1:"), "🤖 done 👍");
    assert_eq!(
      emojify(":not_an_emoji: 10:30:00"),
      ":not_an_emoji: 10:30:00"
    );
    assert_eq!(emojify("`:robot:` :robot:"), "`:robot:` 🤖");
    assert_eq!(
      emojify("```\n:robot:\n```\n:robot:"),
      "```\n:robot:\n```\n🤖"
    );
  }

  #[test]
  fn detects_markdown() {
    assert!(looks_like_markdown("# Title"));
    assert!(looks_like_markdown("text with `code`"));
    assert!(looks_like_markdown("- item"));
    assert!(!looks_like_markdown("Plain words, nothing else."));
  }

  #[test]
  fn stacks_run_tip_first_without_the_default_branch() {
    let all = vec![
      workspace(1, "main", None),
      workspace(2, "a", Some("main")),
      workspace(3, "b", Some("a")),
      workspace(4, "c", Some("b")),
      workspace(5, "alone", Some("main")),
    ];
    let stack = workspace_stack(&all, 3, "main").expect("stack");
    let ids: Vec<_> = stack.iter().map(|(w, current)| (w.id, *current)).collect();
    assert_eq!(ids, vec![(4, false), (3, true), (2, false)]);
    assert!(workspace_stack(&all, 5, "main").is_none());
  }

  #[test]
  fn history_drops_the_working_copy_and_its_empty_parent() {
    let mut wc = commit("w", "");
    wc.is_working_copy = true;
    let mut empty = commit("e", "(no description)");
    empty.parent_ids = vec!["r".into()];
    let real = commit("r", "Real work");
    let mut target = commit("t", "On main");
    target.on_target_only = true;
    let log = JjLogResult {
      commits: vec![wc, empty, real, target],
      tentative_working_copy: None,
      target_branch: "main".into(),
      workspace_branch: "feat".into(),
      target_branch_commits: vec![],
      merge_base_id: None,
    };
    let (own, target) = history_commits(&log);
    assert_eq!(
      own.iter().map(|c| c.commit_id.as_str()).collect::<Vec<_>>(),
      ["r"]
    );
    assert_eq!(target.len(), 1);
    assert_eq!(diff_stats(&log.commits), (3, 6));
  }

  #[test]
  fn headlines_fall_back_by_kind() {
    assert_eq!(headline(&commit("a", "Fix\n\nbody"), false), "Fix");
    assert_eq!(
      headline(&commit("a", "(no description)"), true),
      "Working copy"
    );
    assert_eq!(headline(&commit("a", ""), false), "Untitled commit");
  }

  #[test]
  fn counts_unique_conflicts() {
    let paths = vec!["a ".into(), "a".into(), "".into(), "b".into()];
    assert_eq!(conflict_count(&paths), 2);
  }

  #[test]
  fn descriptions_fall_back_to_metadata() {
    let mut ws = workspace(1, "a", None);
    ws.metadata = Some(r#"{"description":"From metadata","title":"Meta"}"#.into());
    assert_eq!(description(&ws).as_deref(), Some("From metadata"));
    assert_eq!(display_title(&ws), "Meta");
    ws.description = Some("Own".into());
    assert_eq!(description(&ws).as_deref(), Some("Own"));
  }
}
