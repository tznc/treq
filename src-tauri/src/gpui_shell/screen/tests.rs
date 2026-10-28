use std::{
  cell::RefCell,
  collections::HashMap,
  rc::Rc,
  sync::{Arc, Mutex},
};

use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext};

use super::*;
use code_data::Agent;

/// A screen in a test window, and the actions it sent.
struct Harness {
  cx: VisualTestContext,
  screen: Entity<Screen>,
  sent: Rc<RefCell<Vec<ScreenAction>>>,
  popups: Rc<RefCell<Vec<crate::gpui_shell::popup::PopupRequest>>>,
}

type Sink = Arc<dyn Fn(&[u8]) + Send + Sync>;
type ExitSink = Arc<dyn Fn(Option<u32>) + Send + Sync>;

/// The backend, as the screens see it, recording what they asked of it.
#[derive(Default)]
struct FakeHost {
  /// The tuning agent's proposed review prompt edits.
  prompt_edits: std::sync::Mutex<Vec<crate::local_db::AgentReviewComment>>,
  /// What the folder picker returns, and why starting a dev container fails.
  picked: Option<String>,
  container_error: Option<String>,
  /// The Overview screen's machines and recent repositories.
  listing: machines::Listing,
  /// The dev containers Docker lists, which join the listing.
  containers: (Vec<machines::Machine>, Vec<machines::Recent>),
  /// What a host machine says is installed on it.
  host_tools: Option<Result<Vec<crate::core::PrerequisiteStatus>, String>>,
  /// What a host says holds its terminals (`treq pty-remote status`).
  terminal_status: Option<Result<crate::core::pty_remote_supervisor::TerminalStatus, String>>,
  /// What `check_prerequisites` reports.
  installed: Vec<(&'static str, bool)>,
  /// App and repository settings, by key.
  settings: Mutex<settings_data::Values>,
  /// What `.treq/config.yaml` sets.
  config: crate::repo_config::RepoConfig,
  setup_script: String,
  /// The skill registry, as `skill_catalog` returns it.
  skills: Vec<crate::core::skills::SkillCatalogSkillView>,
  /// Installed skills the registry no longer lists.
  uncataloged: Vec<crate::core::skills::InstalledSkill>,
  /// User-owned skills in the repository and the home folder.
  custom: Vec<crate::core::skills::CustomSkill>,
  /// What the workspace header reads.
  loaded: workspace_data::Loaded,
  pr: workspace_data::PrCache,
  /// What the merge preview reads, and whether merging fails.
  merge: merge::Preview,
  merge_error: Option<String>,
  /// Notes a merge reports, such as stacked PRs moved onto a new base.
  merge_notes: Vec<String>,
  /// What the Code tab reads.
  overview: code_data::Overview,
  /// The workspace setup script's status the Code tab reads, and whether
  /// the Logs preview is on.
  setup_status: Option<crate::core::SetupScriptStatus>,
  logs_preview: bool,
  history: Option<crate::jj::JjLogResult>,
  workspaces: Vec<crate::local_db::Workspace>,
  files: Vec<String>,
  /// What the Commits tab reads.
  diff: Option<crate::jj::JjRevisionDiff>,
  /// Deferred file diffs `commit_file_diff` loads.
  file_diffs: Vec<crate::jj::JjFileDiff>,
  description: String,
  /// Which of the workspace's fields follow its PR.
  pr_sync: Option<crate::local_db::PrSyncRecord>,
  /// The PR's title and body as GitHub has them.
  pr_text: Option<(String, String)>,
  /// What the Changes tab reads, and the repository's `.gitattributes`.
  working: Option<crate::jj::JjRevisionDiff>,
  /// Replaces `working` once set: the files edited on disk meanwhile.
  edited: Mutex<Option<crate::jj::JjRevisionDiff>>,
  /// The token hunk reads report instead of the diff's: the disk moved on
  /// between the two reads.
  hunks_token: Mutex<Option<String>>,
  gitattributes: String,
  /// The kept review's comments JSON.
  review: Mutex<Option<String>>,
  /// The pull request's review threads and the review agent's comments.
  threads: Vec<crate::github::GhReviewThread>,
  agent: Mutex<Vec<crate::local_db::AgentReviewComment>>,
  /// What the PR review reads: the remote's `owner/repo`, the PR's diff and
  /// the signed-in login. Its drafts are `agent`.
  pr_remote: Option<String>,
  pr_diff: Option<Result<String, String>>,
  pr_viewer: Option<String>,
  /// Why submitting the review fails, when it does.
  pr_submit_error: Option<String>,
  /// What the Logs tab reads.
  logs: Vec<crate::core::checks_logs::LogRecordView>,
  /// What the Logs tab's chart reads.
  buckets: Vec<crate::core::checks_logs::LogBucket>,
  workflows: Vec<crate::core::checks::WorkflowInfo>,
  chats: Vec<crate::core::agent_chat::AgentChat>,
  sql: Option<Result<crate::core::checks_logs::SqlResult, String>>,
  /// What the terminal pane reads, and the PTYs it started.
  sessions: Vec<crate::local_db::Session>,
  /// The supporting repositories of every repository.
  supporting: Vec<String>,
  /// The devcontainers preview.
  devcontainers: bool,
  ptys: Mutex<Vec<String>>,
  /// The sizes PTYs were given, as (id, cols, rows).
  resizes: Mutex<Vec<(String, u16, u16)>>,
  /// PTY output listeners, by PTY id.
  sinks: Mutex<Vec<(String, Sink)>>,
  /// Remote session exit listeners, by local id.
  exits: Mutex<Vec<(String, ExitSink)>>,
  /// What `PtyList` says of a remote session (`None`: the host is down).
  remote_running: Mutex<Option<bool>>,
  /// The SSH host refuses attaches from now on.
  attach_down: Mutex<bool>,
  /// What `treq agent msg` has waiting for each terminal.
  msg_waiting: Mutex<HashMap<String, terminal_data::WaitingMessages>>,
  /// What the dialogs read: the prompt history, and the stashes and their diff.
  prompts: Vec<crate::local_db::PromptHistoryEntry>,
  stash_list: Mutex<Vec<crate::local_db::StashEntry>>,
  stash_diff: Option<crate::jj::JjRevisionDiff>,
  /// Whether applying a stash fails.
  apply_error: Option<String>,
  /// What the remote dialogs read: the `~/.ssh` keys, the host key a
  /// machine presents (default `SHA256:abc123`), the alias `~/.ssh/config`
  /// resolves and the sessions running on the endpoint.
  ssh_keys: Vec<crate::core::remote_local_keys::LocalSshIdentity>,
  host_key: Option<Result<String, String>>,
  resolved_alias: Option<Result<crate::core::remote_ssh_config::ResolvedSshAlias, String>>,
  remote_sessions: Option<Result<Vec<crate::core::pty_remote_supervisor::PtySessionInfo>, String>>,
  /// What the workspace dialogs read: commits ahead, branches, the home
  /// branch, branches that exist locally or on the remote, and symlink
  /// suggestions.
  ahead: Vec<crate::core::WorkspaceCommit>,
  branches: Vec<crate::jj::JjBranch>,
  home_branch: Option<String>,
  local_branches: Vec<String>,
  remote_branches: Vec<String>,
  suggestions: Vec<String>,
  /// Workspace dialog calls that fail, by name (`"new_workspace"`, ...).
  failing: Vec<&'static str>,
  /// Workspaces the dialogs created.
  created: Mutex<Vec<crate::local_db::Workspace>>,
  /// What the Linear view reads.
  linear: linear_view::Fixture,
  /// What the file browser reads: directory listings and file texts by
  /// full path, and its kept review's comments JSON.
  dirs: HashMap<String, Vec<crate::core::WorkspaceEntry>>,
  file_texts: HashMap<String, String>,
  browser_review: Mutex<Option<String>>,
  /// What the GitHub view reads.
  github: github::Fake,
  /// What the Open Workspaces dialog reads.
  open_workspaces: open_workspaces::Fake,
  /// What the account reads: the GitHub repositories and Linear's OAuth
  /// page (`Err` refuses it).
  github_repos: Option<Result<Vec<crate::core::account::GithubRepository>, String>>,
  oauth: Option<Result<String, crate::core::supabase::ApiError>>,
  /// Why deleting the account fails; `None` deletes it.
  account_deletion: Option<crate::core::supabase::ApiError>,
  /// What the agent launches read and did: each new session's start data
  /// by pane id, the `treq send` attachments, the prompt dialog's tasks
  /// (and whether starting fails), and the sessions started.
  pending: Mutex<HashMap<i64, launch_data::PendingSession>>,
  send: Mutex<Vec<launch_data::SendAsset>>,
  tasks: Mutex<Vec<launch_data::TaskRequest>>,
  task_error: Option<String>,
  launches: Mutex<Vec<(launch_data::LaunchRequest, bool)>>,
  started: Mutex<Vec<(launch_data::SessionStart, bool)>>,
  /// The workspace actions are React's (a remote repository).
  react_actions: bool,
  calls: Mutex<Vec<String>>,
}

impl file_browser_data::FileBrowserHost for FakeHost {
  fn list_directory(&self, path: &str) -> Result<Vec<crate::core::WorkspaceEntry>, String> {
    self.record(format!("read dir {path}"));
    self
      .dirs
      .get(path)
      .cloned()
      .ok_or_else(|| format!("no dir {path}"))
  }

  fn read_file(&self, path: &str) -> Result<String, String> {
    self.record(format!("read file {path}"));
    self
      .file_texts
      .get(path)
      .cloned()
      .ok_or_else(|| format!("no file {path}"))
  }

  fn modified_at(&self, _: &str) -> Option<String> {
    Some("2024-01-02T03:04:05Z".into())
  }

  fn load_browser_review(
    &self,
    _: &str,
    workspace_id: i64,
  ) -> Result<Option<crate::local_db::FileBrowserPendingReview>, String> {
    self.record(format!("read browser review {workspace_id}"));
    Ok(self.browser_review.lock().unwrap().clone().map(|comments| {
      crate::local_db::FileBrowserPendingReview {
        id: 1,
        workspace_id,
        comments,
        summary_text: None,
        created_at: String::new(),
        updated_at: String::new(),
      }
    }))
  }

  fn save_browser_review(
    &self,
    _: &str,
    workspace_id: i64,
    comments: &str,
    summary: Option<&str>,
  ) -> Result<(), String> {
    self.record(format!("save browser review {workspace_id} {summary:?}"));
    *self.browser_review.lock().unwrap() = Some(comments.to_string());
    Ok(())
  }

  fn clear_browser_review(&self, _: &str, workspace_id: i64) -> Result<(), String> {
    self.record(format!("clear browser review {workspace_id}"));
    *self.browser_review.lock().unwrap() = None;
    Ok(())
  }
}

impl FakeHost {
  /// The copy a customise call reports, named after the registry skill.
  fn customize_copy(&self, id: &str) -> Result<crate::core::skills::CustomizedSkill, String> {
    let name = self
      .skills
      .iter()
      .find(|s| s.skill.id == id)
      .map_or_else(|| id.to_string(), |s| s.skill.name.clone());
    Ok(crate::core::skills::CustomizedSkill {
      id: id.into(),
      agents_skill_path: format!(".agents/skills/{name}"),
      claude_skill_path: format!(".claude/skills/{name}"),
      name,
    })
  }

  /// What the Changes tab reads now.
  fn working(&self) -> Option<crate::jj::JjRevisionDiff> {
    let edited = self.edited.lock().unwrap().clone();
    edited.or_else(|| self.working.clone())
  }

  fn calls(&self) -> Vec<String> {
    self.calls.lock().unwrap().clone()
  }

  fn record(&self, call: String) {
    self.calls.lock().unwrap().push(call);
  }
}

impl FakeHost {
  fn with_settings(pairs: &[(&str, &str)]) -> Self {
    Self {
      settings: Mutex::new(
        pairs
          .iter()
          .map(|(k, v)| (k.to_string(), Some(v.to_string())))
          .collect(),
      ),
      ..Self::default()
    }
  }

  fn setting(&self, key: &str) -> Option<String> {
    self.settings.lock().unwrap().get(key).cloned().flatten()
  }

  /// The calls that change something, without the reads.
  fn writes(&self) -> Vec<String> {
    self
      .calls()
      .into_iter()
      .filter(|c| !c.starts_with("check") && !c.starts_with("read"))
      .collect()
  }
}

impl machines::MachinesHost for FakeHost {
  fn listing(&self) -> machines::Listing {
    self.listing.clone()
  }

  fn containers(&self) -> (Vec<machines::Machine>, Vec<machines::Recent>) {
    self.containers.clone()
  }

  fn machine_tools(
    &self,
    place: &machines::Place,
  ) -> Result<Vec<crate::core::PrerequisiteStatus>, String> {
    if *place == machines::Place::Local {
      return Ok(checklist::ToolsHost::check_prerequisites(self));
    }
    self.record(format!("tools {place:?}"));
    self
      .host_tools
      .clone()
      .unwrap_or_else(|| Err("ssh: connection refused".into()))
  }

  fn open_local(&self, path: &str) {
    self.record(format!("open_local {path}"));
  }

  fn remove_machine(&self, place: &machines::Place) -> bool {
    self.record(format!("remove {place:?}"));
    true
  }

  fn set_terminal_backend(
    &self,
    place: &machines::Place,
    backend: crate::core::pty_remote_supervisor::PtyBackend,
  ) -> Result<(), String> {
    self.record(format!("terminals {place:?} {backend:?}"));
    Ok(())
  }

  fn reset_terminals(&self, place: &machines::Place) -> Result<(), String> {
    self.record(format!("reset terminals {place:?}"));
    Ok(())
  }

  fn terminal_status(
    &self,
    place: &machines::Place,
  ) -> Result<crate::core::pty_remote_supervisor::TerminalStatus, String> {
    self.record(format!("read terminal status {place:?}"));
    self
      .terminal_status
      .clone()
      .unwrap_or_else(|| Err("ssh: connection refused".into()))
  }

  fn pick_folder(&self) -> Option<String> {
    self.picked.clone()
  }

  fn start_dev_container(&self, repo_path: &str) -> Result<(), String> {
    self.record(format!("container up {repo_path}"));
    self.container_error.clone().map_or(Ok(()), Err)
  }
}

impl onboarding::OnboardingHost for FakeHost {}

impl settings_data::SettingsHost for FakeHost {
  fn settings(&self, keys: &[String]) -> Result<settings_data::Values, String> {
    self.record("read settings".into());
    Ok(keys.iter().map(|k| (k.clone(), self.setting(k))).collect())
  }

  fn set_settings(&self, values: &[(String, String)]) -> Result<(), String> {
    let pairs: Vec<_> = values.iter().map(|(k, v)| format!("{k}={v}")).collect();
    self.record(format!("set {}", pairs.join(" ")));
    let mut settings = self.settings.lock().unwrap();
    for (k, v) in values {
      settings.insert(k.clone(), Some(v.clone()));
    }
    Ok(())
  }

  fn setup_script(&self) -> Result<crate::core::app_setup_script::AppSetupScriptStatus, String> {
    self.record("read setup".into());
    Ok(crate::core::app_setup_script::AppSetupScriptStatus {
      script: self.setup_script.clone(),
      always_run: false,
      last_run_at: None,
      last_status: None,
      running: false,
    })
  }

  fn save_setup_script(&self, script: &str, always_run: bool) -> Result<(), String> {
    self.record(format!("save setup {script:?} {always_run}"));
    Ok(())
  }

  fn run_setup_script(&self) {
    self.record("run setup".into());
  }

  fn repo_config(&self, repo_path: &str) -> Result<crate::repo_config::RepoConfig, String> {
    self.record(format!("read config {repo_path}"));
    Ok(self.config.clone())
  }

  fn repo_settings(
    &self,
    repo_path: &str,
    keys: &[String],
  ) -> Result<settings_data::Values, String> {
    self.record(format!("read repo {repo_path}"));
    Ok(
      keys
        .iter()
        .map(|k| (k.clone(), self.setting(&format!("{repo_path}:{k}"))))
        .collect(),
    )
  }

  fn set_repo_settings(&self, repo_path: &str, values: &[(String, String)]) -> Result<(), String> {
    let pairs: Vec<_> = values.iter().map(|(k, v)| format!("{k}={v}")).collect();
    self.record(format!("set repo {repo_path} {}", pairs.join(" ")));
    let mut settings = self.settings.lock().unwrap();
    for (k, v) in values {
      settings.insert(format!("{repo_path}:{k}"), Some(v.clone()));
    }
    Ok(())
  }

  fn skill_catalog(
    &self,
    _: Option<&str>,
  ) -> Result<Vec<crate::core::skills::SkillCatalogSkillView>, String> {
    self.record("read skills".into());
    Ok(self.skills.clone())
  }

  fn installed_skills(
    &self,
    _: Option<&str>,
  ) -> Result<Vec<crate::core::skills::InstalledSkill>, String> {
    Ok(
      self
        .skills
        .iter()
        .filter_map(|s| s.installed.clone())
        .chain(self.uncataloged.iter().cloned())
        .collect(),
    )
  }

  fn install_skill(
    &self,
    skill_id: &str,
    scope: crate::core::skills::SkillInstallScope,
    repo_path: Option<&str>,
  ) -> Result<(), String> {
    self.record(format!("install {skill_id} {scope:?} {repo_path:?}"));
    Err("checksum mismatch".into())
  }

  fn uninstall_skill(&self, skill_id: &str, _: Option<&str>) -> Result<(), String> {
    self.record(format!("uninstall {skill_id}"));
    Ok(())
  }

  fn set_skill_scope(
    &self,
    skill_id: &str,
    scope: crate::core::skills::SkillInstallScope,
    _: Option<&str>,
  ) -> Result<(), String> {
    self.record(format!("scope {skill_id} {scope:?}"));
    Ok(())
  }

  fn custom_skills(&self, _: Option<&str>) -> Vec<crate::core::skills::CustomSkill> {
    self.custom.clone()
  }

  fn customize_skill(
    &self,
    skill_id: &str,
    repo_path: &str,
  ) -> Result<crate::core::skills::CustomizedSkill, String> {
    self.record(format!("customize {skill_id} {repo_path}"));
    self.customize_copy(skill_id)
  }

  fn customize_registry_skill(
    &self,
    skill_id: &str,
    repo_path: &str,
  ) -> Result<crate::core::skills::CustomizedSkill, String> {
    self.record(format!("customize-registry {skill_id} {repo_path}"));
    self.customize_copy(skill_id)
  }

  fn customize_global_skill(
    &self,
    name: &str,
    source: &str,
    repo_path: &str,
  ) -> Result<crate::core::skills::CustomizedSkill, String> {
    self.record(format!("customize-global {name} {source} {repo_path}"));
    self.customize_copy(name)
  }

  fn snapshot_review_prompt(&self, repo_path: &str, custom: &str) -> Result<String, String> {
    self.record(format!("snapshot review prompt {repo_path} {custom:?}"));
    Ok(format!("{repo_path}/.treq/review-prompt"))
  }

  fn prompt_edits(&self, _: &str) -> Result<Vec<crate::local_db::AgentReviewComment>, String> {
    Ok(self.prompt_edits.lock().unwrap().clone())
  }

  fn apply_prompt_edit(&self, _: &str, id: &str, _: &str) -> Result<String, String> {
    self.record(format!("apply prompt edit {id}"));
    self.prompt_edits.lock().unwrap().retain(|c| c.id != id);
    Ok(format!("applied {id}"))
  }

  fn delete_prompt_edit(&self, _: &str, id: &str) -> Result<(), String> {
    self.record(format!("delete prompt edit {id}"));
    self.prompt_edits.lock().unwrap().retain(|c| c.id != id);
    Ok(())
  }

  fn fetch_image(&self, url: &str) -> Result<Vec<u8>, String> {
    self.record(format!("fetch {url}"));
    Err("offline".into())
  }

  fn notify(&self, event: crate::gpui_shell::native::NativeEvent) {
    let json = serde_json::to_value(&event).expect("serialize");
    let mut line = format!("notify {}", json["kind"].as_str().unwrap_or_default());
    if let Some(title) = json["title"].as_str() {
      line += &format!(" {title}");
    }
    self.record(line);
  }
}

impl workspace_data::WorkspaceHost for FakeHost {
  fn load(&self, route: &workspace_data::Route) -> workspace_data::Loaded {
    self.record(format!("read workspace {}", route.revision));
    let mut loaded = self.loaded.clone();
    // Another workspace of the list than the one `loaded` is for.
    let listed = |id| self.workspaces.iter().find(|w| w.id == id).cloned();
    match (route.workspace_id, loaded.workspace.as_ref().map(|w| w.id)) {
      (Some(id), Some(loaded_id)) if id != loaded_id => loaded.workspace = listed(id),
      _ => {}
    }
    loaded
  }

  fn pr_cache(&self, _: &str, branch: &str) -> workspace_data::PrCache {
    self.record(format!("read pr {branch}"));
    self.pr.clone()
  }

  fn push(&self, _: &str, workspace_id: Option<i64>) -> workspace_data::Outcome {
    self.record(format!("push {workspace_id:?}"));
    workspace_data::Outcome::ok("Pushed to remote", "Changes pushed".into())
  }

  fn sync(&self, _: &str, workspace_id: Option<i64>) -> workspace_data::Outcome {
    self.record(format!("sync {workspace_id:?}"));
    workspace_data::Outcome::failed("Sync failed", "offline".into())
  }

  fn reset_to_remote(&self, _: &str, workspace_id: i64) -> workspace_data::Outcome {
    self.record(format!("reset {workspace_id}"));
    workspace_data::Outcome::ok("Reset to remote", "Dropped 2 local commits".into())
  }

  fn rebase_home(&self, _: &str, branch: &str) -> workspace_data::Outcome {
    self.record(format!("rebase {branch}"));
    workspace_data::Outcome::ok("Rebase complete", "Branch rebased onto target".into())
  }

  fn notify(&self, event: crate::gpui_shell::native::NativeEvent) {
    settings_data::SettingsHost::notify(self, event);
  }
}

impl launch_data::LaunchHost for FakeHost {
  fn start_agent_task(&self, task: &launch_data::TaskRequest) -> Result<bool, String> {
    self.record("start task".into());
    self.tasks.lock().unwrap().push(task.clone());
    if let Some(err) = &self.task_error {
      return Err(err.clone());
    }
    Ok(!task.text.trim().is_empty() || task.issue.is_some())
  }

  fn launch_session(
    &self,
    request: &launch_data::LaunchRequest,
    select: bool,
  ) -> Result<launch_data::SessionStart, String> {
    self.record(format!(
      "launch {:?} {:?} {:?}",
      request.session_name, request.mode, request.agent
    ));
    self
      .launches
      .lock()
      .unwrap()
      .push((request.clone(), select));
    Ok(launch_data::SessionStart {
      session_id: 41,
      repo_path: request.repo_path.clone(),
      workspace_id: request.workspace_id,
      pending_prompt: request.prompt.clone(),
      ..Default::default()
    })
  }

  fn session_started(&self, start: &launch_data::SessionStart, select: bool) {
    self.started.lock().unwrap().push((start.clone(), select));
  }

  fn resolve_agent(&self, _: &str, agent: Option<&str>) -> Option<String> {
    agent.map(str::to_string)
  }

  fn pending_session(&self, pane_id: i64) -> Option<launch_data::PendingSession> {
    self.pending.lock().unwrap().get(&pane_id).cloned()
  }

  fn send_assets(&self) -> Vec<launch_data::SendAsset> {
    self.send.lock().unwrap().clone()
  }

  fn dismiss_send_asset(&self, id: &str) {
    self.record(format!("dismiss asset {id}"));
    self.send.lock().unwrap().retain(|a| a.id != id);
  }

  fn record_review(&self, _: &str, workspace_id: Option<i64>, scope: &str, _: &str, mode: &str) {
    self.record(format!("record review {workspace_id:?} {scope} {mode}"));
  }
}

impl FakeHost {
  /// The sessions the screens started, as `SessionCreationInfo` JSON.
  fn started_sessions(&self) -> Vec<serde_json::Value> {
    self
      .started
      .lock()
      .unwrap()
      .iter()
      .map(|(s, _)| serde_json::to_value(s).unwrap())
      .collect()
  }
}

impl crate::gpui_shell::popover::PopoverHost for FakeHost {
  fn starting_prompt(
    &self,
    _: &str,
    workspace_id: i64,
  ) -> Option<crate::gpui_shell::popover::data::StartingPrompt> {
    self.record(format!("read starting prompt {workspace_id}"));
    None
  }

  fn branches(&self, repo_path: &str) -> Result<Vec<crate::jj::JjBranch>, String> {
    self.record(format!("list branches {repo_path}"));
    Ok(Vec::new())
  }
}

impl FakeHost {
  fn merge_result(&self) -> Result<crate::core::MergeResult, String> {
    match &self.merge_error {
      Some(err) => Err(err.clone()),
      None => Ok(crate::core::MergeResult {
        plan: self
          .merge
          .plan
          .clone()
          .unwrap_or_else(|| merge_plan(&[], false)),
        notes: self.merge_notes.clone(),
      }),
    }
  }
}

impl merge::MergeHost for FakeHost {
  fn merge_preview(&self, _: &str, workspace_id: i64, rollup: bool) -> merge::Preview {
    self.record(format!(
      "read merge {workspace_id}{}",
      if rollup { " rollup" } else { "" }
    ));
    self.merge.clone()
  }

  fn merge(
    &self,
    _: &str,
    workspace_id: i64,
    message: &str,
    strategy: merge::Strategy,
    options: crate::core::LocalMergeOptions,
  ) -> Result<crate::core::MergeResult, String> {
    let mut line = format!("merge {workspace_id} {strategy:?} {message:?}");
    if options.rollup {
      line.push_str(" rollup");
    }
    if options.push {
      line.push_str(" push");
    }
    self.record(line);
    self.merge_result()
  }

  fn merge_on_github(
    &self,
    _: &str,
    workspace_id: i64,
    request: &merge::GithubMerge,
  ) -> Result<crate::core::MergeResult, String> {
    self.record(format!(
      "merge on github {workspace_id} {}#{} {:?} {:?}",
      request.repo_full_name, request.pr_number, request.method, request.subject
    ));
    self.merge_result()
  }

  fn sync_merged(&self, _: &str, workspace_id: i64) -> Result<crate::core::MergeResult, String> {
    self.record(format!("sync merged {workspace_id}"));
    self.merge_result()
  }
}

impl code_data::CodeHost for FakeHost {
  fn overview(&self, _: &str, workspace_id: Option<i64>) -> Result<code_data::Overview, String> {
    self.record(format!("read overview {workspace_id:?}"));
    if self.failing.contains(&"remote_overview") {
      return Err(crate::gpui_shell::remote_repo::unsupported(
        "Browsing the file tree",
      ));
    }
    Ok(self.overview.clone())
  }

  fn commits(
    &self,
    _: &str,
    workspace_id: Option<i64>,
    include_target: bool,
    limit: Option<usize>,
  ) -> Result<crate::jj::JjLogResult, String> {
    self.record(format!(
      "read commits {workspace_id:?} {include_target} {limit:?}"
    ));
    self.history.clone().ok_or_else(|| "no history".into())
  }

  fn workspaces(&self, _: &str) -> Result<Vec<crate::local_db::Workspace>, String> {
    self.record("read workspaces".into());
    Ok(self.workspaces.clone())
  }

  fn set_submodule_synced(&self, _: &str, path: &str, enabled: bool) -> Result<(), String> {
    self.record(format!("sync submodule {path} {enabled}"));
    Ok(())
  }

  fn set_description(&self, _: &str, workspace_id: i64, description: &str) -> Result<(), String> {
    self.record(format!("set description {workspace_id} {description:?}"));
    Ok(())
  }

  fn set_title(&self, _: &str, workspace_id: i64, title: &str) -> Result<(), String> {
    self.record(format!("set title {workspace_id} {title:?}"));
    Ok(())
  }

  /// A local host: the file index is in memory, so searches run at once.
  fn file_search_delay(&self, _: &str) -> std::time::Duration {
    std::time::Duration::ZERO
  }

  fn warm_file_index(&self, _: &str, workspace_id: Option<i64>, _: &str) {
    self.record(format!("read file index {workspace_id:?}"));
  }

  fn pr_sync(&self, _: &str, _: i64) -> Option<crate::local_db::PrSyncRecord> {
    self.record("read pr sync".into());
    self.pr_sync.clone()
  }

  fn resync_from_pr(&self, _: &str, workspace_id: i64) -> Result<(), String> {
    self.record(format!("resync from PR {workspace_id}"));
    Ok(())
  }

  fn pr_text(&self, _: &str, _: &crate::local_db::Workspace) -> Option<(String, String)> {
    self.record("read pr text".into());
    self.pr_text.clone()
  }

  fn push_to_pr(&self, _: &str, workspace_id: i64) -> Result<(), String> {
    self.record(format!("push to PR {workspace_id}"));
    Ok(())
  }

  fn search_files(
    &self,
    _: &str,
    _: Option<i64>,
    _: &str,
    query: &str,
    limit: usize,
  ) -> Result<Vec<String>, String> {
    self.record(format!("read files {query:?}"));
    Ok(
      self
        .files
        .iter()
        .filter(|f| !query.is_empty() && f.contains(query))
        .take(limit)
        .cloned()
        .collect(),
    )
  }

  fn devcontainers_enabled(&self) -> bool {
    self.devcontainers
  }

  fn devcontainer_status(&self, _: &str) -> crate::core::devcontainer::DevcontainerStatus {
    crate::core::devcontainer::DevcontainerStatus {
      configured: false,
      config_path: None,
      cli_available: false,
      running: false,
    }
  }

  fn devcontainer_up(
    &self,
    repo_path: &str,
  ) -> Result<crate::core::devcontainer::DevcontainerStatus, String> {
    self.record("devcontainer up".into());
    Ok(self.devcontainer_status(repo_path))
  }

  fn branch_names(&self, _: &str) -> Result<std::collections::HashSet<String>, String> {
    self.record("read branches".into());
    Ok(["treq/fix-login".to_string()].into())
  }

  fn create_workspace(&self, _: &str, branch: &str, title: &str) -> Result<i64, String> {
    self.record(format!("create workspace {branch} {title:?}"));
    Ok(9)
  }

  fn create_session(&self, _: &str, workspace_id: Option<i64>, name: &str) -> Result<i64, String> {
    self.record(format!("create session {workspace_id:?} {name:?}"));
    Ok(31)
  }

  fn pick_files(&self) -> Option<Vec<String>> {
    None
  }

  fn setup(&self, _: &str, workspace_id: i64) -> Option<crate::core::SetupScriptStatus> {
    self.record(format!("read setup {workspace_id}"));
    self.setup_status.clone()
  }

  fn logs_enabled(&self) -> bool {
    self.logs_preview
  }

  fn run_setup(
    &self,
    target: &crate::core::running_checks::ChecksTarget,
  ) -> Result<Vec<crate::core::checks::JobResult>, String> {
    self.record(format!("run setup {}", target.workspace_id));
    Ok(vec![])
  }
}

impl changes_data::ChangesHost for FakeHost {
  fn changed_files(
    &self,
    _: &str,
    workspace_id: Option<i64>,
  ) -> Result<Vec<crate::jj::JjFileChange>, String> {
    self.record(format!("read changed files {workspace_id:?}"));
    let working = self.working().ok_or("no changes")?;
    Ok(working.uncommitted_files)
  }

  fn workspace_diff(
    &self,
    _: &str,
    workspace_id: i64,
  ) -> Result<crate::jj::JjRevisionDiff, String> {
    self.record(format!("read workspace diff {workspace_id}"));
    self.working().ok_or_else(|| "no changes".into())
  }

  fn file_hunks(
    &self,
    _: &str,
    _: Option<i64>,
    paths: Vec<String>,
    token: Option<&str>,
  ) -> Result<crate::core::WorkspaceFileHunksBatch, String> {
    self.record(format!("read hunks {paths:?}"));
    if let Some(token) = token {
      self.record(format!("hunks at {token}"));
    }
    // Borrowed, not cloned: a batch of a large changeset would copy it all.
    let edited = self.edited.lock().unwrap();
    let working = edited
      .as_ref()
      .or(self.working.as_ref())
      .ok_or("no changes")?;
    // As the backend: the diff `token` names, unless a test moved the disk.
    let read_at = self.hunks_token.lock().unwrap().clone();
    Ok(crate::core::WorkspaceFileHunksBatch {
      snapshot_token: read_at
        .or_else(|| token.map(str::to_string))
        .or_else(|| working.working_copy_token.clone())
        .unwrap_or_default(),
      files: paths
        .iter()
        .map(|path| crate::core::WorkspaceFileHunksBatchFile {
          path: path.clone(),
          hunks: working
            .hunks_by_file
            .iter()
            .find(|f| &f.path == path)
            .map(|f| f.hunks.clone())
            .unwrap_or_default(),
          error: None,
        })
        .collect(),
    })
  }

  fn file_lines(
    &self,
    _: &str,
    _: Option<i64>,
    path: &str,
    start: usize,
    end: usize,
  ) -> Result<Vec<String>, String> {
    self.record(format!("read lines {path} {start}..{end}"));
    Ok((start..=end).map(|n| format!("line {n}")).collect())
  }

  fn generated_files(&self, _: &str, _: Option<i64>) -> crate::core::generated::GeneratedFiles {
    crate::core::generated::GeneratedFiles::parse(&self.gitattributes)
  }

  fn discard_file(&self, _: &str, path: &str) -> Result<(), String> {
    self.record(format!("discard {path}"));
    Ok(())
  }

  fn commit(&self, _: &str, workspace_id: Option<i64>, message: &str) -> Result<String, String> {
    self.record(format!("commit {workspace_id:?} {message:?}"));
    Ok("c1".into())
  }

  fn split(&self, _: &str, message: &str, paths: Vec<String>) -> Result<String, String> {
    self.record(format!("split {message:?} {paths:?}"));
    Ok("c2".into())
  }

  fn push_remote(&self, _: &str, workspace_id: Option<i64>) -> Result<String, String> {
    self.record(format!("push {workspace_id:?}"));
    Ok(String::new())
  }

  fn stash_all(
    &self,
    _: &str,
    workspace_id: Option<i64>,
  ) -> Result<crate::local_db::StashEntry, String> {
    self.record(format!("stash all {workspace_id:?}"));
    Ok(crate::local_db::StashEntry {
      id: 1,
      workspace_id,
      workspace_label: "login".into(),
      commit_id: "abc".into(),
      change_id: "xyz".into(),
      short_commit_id: "abc".into(),
      bookmark_name: "treq/stash".into(),
      created_at: String::new(),
      additions: 1,
      deletions: 0,
      files_changed: vec![],
    })
  }

  fn mark_viewed(&self, _: &str, path: &str, hash: &str) -> Result<(), String> {
    self.record(format!("mark viewed {path} {hash}"));
    Ok(())
  }

  fn unmark_viewed(&self, _: &str, path: &str) -> Result<(), String> {
    self.record(format!("unmark viewed {path}"));
    Ok(())
  }

  fn github_remote(&self, _: &str) -> bool {
    false
  }

  fn editor_apps(&self) -> changes_data::EditorApps {
    changes_data::EditorApps::default()
  }

  fn reveal(&self, path: &str) {
    self.record(format!("reveal {path}"));
  }

  fn load_review(
    &self,
    _: &str,
    workspace_id: i64,
  ) -> Result<Option<crate::local_db::PendingReview>, String> {
    self.record(format!("read review {workspace_id}"));
    Ok(
      self
        .review
        .lock()
        .unwrap()
        .clone()
        .map(|comments| crate::local_db::PendingReview {
          id: 1,
          workspace_id,
          comments,
          viewed_files: None,
          summary_text: None,
          conflict_comments: None,
          created_at: String::new(),
          updated_at: String::new(),
        }),
    )
  }

  fn save_review(
    &self,
    _: &str,
    workspace_id: i64,
    comments: &str,
    summary: Option<&str>,
    _: Option<&str>,
  ) -> Result<(), String> {
    self.record(format!("save review {workspace_id} {summary:?}"));
    *self.review.lock().unwrap() = Some(comments.to_string());
    Ok(())
  }

  fn clear_review(&self, _: &str, workspace_id: i64) -> Result<(), String> {
    self.record(format!("clear review {workspace_id}"));
    *self.review.lock().unwrap() = None;
    Ok(())
  }

  fn review_threads(
    &self,
    _: &str,
    branch: &str,
  ) -> Result<Vec<crate::github::GhReviewThread>, String> {
    self.record(format!("read threads {branch}"));
    Ok(self.threads.clone())
  }

  fn react(&self, comment_id: &str, content: &str, on: bool) -> Result<(), String> {
    self.record(format!("react {comment_id} {content} {on}"));
    Ok(())
  }

  fn agent_comments(
    &self,
    _: &str,
    workspace_id: i64,
  ) -> Result<Vec<crate::local_db::AgentReviewComment>, String> {
    self.record(format!("read agent comments {workspace_id}"));
    Ok(self.agent.lock().unwrap().clone())
  }

  fn resolve_agent_comment(&self, _: &str, id: &str) -> Result<(), String> {
    self.record(format!("resolve agent comment {id}"));
    for c in self.agent.lock().unwrap().iter_mut().filter(|c| c.id == id) {
      c.status = "resolved".into();
    }
    Ok(())
  }

  fn delete_agent_comment(&self, _: &str, id: &str) -> Result<(), String> {
    self.record(format!("delete agent comment {id}"));
    self.agent.lock().unwrap().retain(|c| c.id != id);
    Ok(())
  }

  fn apply_suggestion(
    &self,
    workspace_path: &str,
    path: &str,
    (start, end): (u32, u32),
    original: &[String],
    suggested: &str,
  ) -> Result<(), String> {
    self.record(format!(
      "apply suggestion {workspace_path} {path} {start}..{end} {original:?} -> {suggested:?}"
    ));
    Ok(())
  }

  fn apply_agent_suggestion(&self, _: &str, id: &str) -> Result<(), String> {
    self.record(format!("apply agent suggestion {id}"));
    Ok(())
  }
}

impl terminal_data::TerminalHost for FakeHost {
  fn spawn_pty(&self, spawn: &terminal_data::Spawn) -> Result<(), String> {
    self.record(format!(
      "spawn {} in {:?} running {:?}",
      spawn.id,
      spawn.working_dir,
      spawn
        .auto_command
        .as_deref()
        .map(|c| c.split(' ').next().unwrap_or_default().to_string())
    ));
    self.ptys.lock().unwrap().push(spawn.id.clone());
    Ok(())
  }

  fn pty_exists(&self, id: &str) -> bool {
    self.ptys.lock().unwrap().iter().any(|p| p == id)
  }

  fn write_pty(&self, id: &str, data: &str) -> Result<(), String> {
    if self.failing.contains(&"write_pty") {
      return Err("channel closed".into());
    }
    self.record(format!("write {id} {data:?}"));
    Ok(())
  }

  fn resize_pty(&self, id: &str, rows: u16, cols: u16) -> Result<(), String> {
    self
      .resizes
      .lock()
      .unwrap()
      .push((id.to_string(), cols, rows));
    Ok(())
  }

  fn close_pty(&self, id: &str) -> Result<(), String> {
    self.record(format!("close {id}"));
    self.ptys.lock().unwrap().retain(|p| p != id);
    Ok(())
  }

  fn listen(&self, id: &str, sink: terminal_data::OutputSink) -> terminal_data::Listening {
    self
      .sinks
      .lock()
      .unwrap()
      .push((id.to_string(), Arc::from(sink)));
    terminal_data::Listening::new(|| {})
  }

  fn listen_exit(
    &self,
    id: &str,
    sink: Box<dyn Fn(Option<u32>) + Send + Sync>,
  ) -> terminal_data::Listening {
    self
      .exits
      .lock()
      .unwrap()
      .push((id.to_string(), Arc::from(sink)));
    terminal_data::Listening::new(|| {})
  }

  fn remote_attach(
    &self,
    id: &str,
    target: &terminal_data::RemoteTarget,
    _: u16,
    _: u16,
  ) -> Result<(), String> {
    self.record(format!("attach {id} to {}", target.label));
    if self.failing.contains(&"remote_attach") || *self.attach_down.lock().unwrap() {
      return Err("ssh: connection refused".into());
    }
    self.ptys.lock().unwrap().push(id.to_string());
    Ok(())
  }

  fn remote_running(&self, target: &terminal_data::RemoteTarget) -> Option<bool> {
    self.record(format!("list {}", target.label));
    *self.remote_running.lock().unwrap()
  }

  fn remote_stop(&self, target: &terminal_data::RemoteTarget) -> Result<(), String> {
    self.record(format!("stop {}", target.label));
    Ok(())
  }

  fn remote_rename(
    &self,
    target: &terminal_data::RemoteTarget,
    title: &str,
  ) -> Result<Option<String>, String> {
    self.record(format!("rename {} to {title}", target.label));
    Ok(crate::core::pty_remote_supervisor::clean_title(title))
  }

  fn rename_session(&self, repo_path: &str, session_id: i64, name: &str) -> Result<(), String> {
    self.record(format!(
      "rename session {session_id} in {repo_path} to {name}"
    ));
    Ok(())
  }

  fn sessions(&self, _: &str) -> Result<Vec<crate::local_db::Session>, String> {
    Ok(self.sessions.clone())
  }

  fn supporting_repos(&self, _: &str) -> Vec<String> {
    self.supporting.clone()
  }

  fn shell_command(&self) -> Option<String> {
    Some("export PATH=\"/bin:$PATH\"".into())
  }

  fn agent_msg_register(&self, session: &crate::core::agent_msg::hub::SessionInfo, _: u16, _: u16) {
    self.record(format!(
      "msg register {} {} {} {:?}",
      session.pty_id, session.handle, session.repo, session.workspace
    ));
  }

  fn agent_msg_register_remote(
    &self,
    pty: &str,
    target: &terminal_data::RemoteTarget,
    _: u16,
    _: u16,
  ) {
    self.record(format!("msg register remote {pty} {}", target.label));
  }

  fn agent_msg_key(&self, pty: &str) {
    self.record(format!("msg key {pty}"));
  }

  fn agent_msg_waiting(&self, pty: &str) -> Option<terminal_data::WaitingMessages> {
    self.msg_waiting.lock().unwrap().get(pty).cloned()
  }

  fn agent_msg_deliver_now(&self, pty: &str) {
    self.record(format!("msg deliver {pty}"));
  }

  fn agent_msg_generation(&self) -> u64 {
    self.msg_waiting.lock().unwrap().len() as u64
  }

  fn prepare_agent(
    &self,
    start: &terminal_data::AgentStart,
  ) -> Result<terminal_data::Prepared, String> {
    self.record(format!(
      "prepare {} {:?}",
      start.agent, start.pending_prompt
    ));
    Ok(terminal_data::Prepared {
      command: format!("{} --start", start.agent),
      skill_write_warning: None,
      file_paths: vec![format!("/tmp/treq-agent-prompt-{}.txt", start.agent)],
    })
  }

  fn register_chat(
    &self,
    start: &terminal_data::AgentStart,
    pty_id: &str,
    name: &str,
  ) -> Result<(), String> {
    self.record(format!(
      "register chat {} {pty_id} {name}",
      start.session_id
    ));
    Ok(())
  }

  fn record_chat_screen(&self, _: &str, session_id: i64, screen: &str) -> Result<(), String> {
    self.record(format!("record chat {session_id} {screen:?}"));
    Ok(())
  }

  fn record_chat_message(
    &self,
    _: &str,
    session_id: i64,
    _: &str,
    text: &str,
  ) -> Result<(), String> {
    self.record(format!("chat message {session_id} {text:?}"));
    Ok(())
  }

  fn remove_session_files(&self, paths: &[String]) {
    self.record(format!("remove files {paths:?}"));
  }

  fn read_send_asset(&self, path: &str) -> Result<String, String> {
    self.record(format!("read asset {path}"));
    if path.ends_with("long.txt") {
      return Ok((1..=3000).map(|n| format!("line {n}\n")).collect());
    }
    Ok("first\nsecond\nthird".into())
  }

  fn write_send_review_image(&self, repo: &str, name: &str, png: &[u8]) -> Result<String, String> {
    self.record(format!("write review image {name} ({} bytes)", png.len()));
    Ok(format!("{repo}/.treq/send/{name}"))
  }

  fn write_pasted_image(&self, bytes: &[u8]) -> Result<String, String> {
    self.record(format!("save pasted image ({} bytes)", bytes.len()));
    Ok("/tmp/treq-pasted-images/paste-1.png".into())
  }
}

impl FakeHost {
  /// Sends `data` as the PTY's output.
  /// Ends the channel of remote session `id`.
  fn exit(&self, id: &str, status: Option<u32>) {
    let sinks: Vec<_> = self
      .exits
      .lock()
      .unwrap()
      .iter()
      .filter(|(pty, _)| pty == id)
      .map(|(_, sink)| sink.clone())
      .collect();
    for sink in sinks {
      sink(status);
    }
  }

  /// The local ids of remote sessions attached so far, in order.
  fn attached(&self) -> Vec<String> {
    self
      .calls()
      .iter()
      .filter_map(|c| c.strip_prefix("attach "))
      .map(|c| c.split(' ').next().unwrap_or_default().to_string())
      .collect()
  }

  fn output(&self, id: &str, data: &str) {
    let sinks: Vec<_> = self
      .sinks
      .lock()
      .unwrap()
      .iter()
      .filter(|(pty, _)| pty == id)
      .map(|(_, sink)| sink.clone())
      .collect();
    for sink in sinks {
      sink(data.as_bytes());
    }
  }
}

impl logs_data::LogsHost for FakeHost {
  fn repo_logs(
    &self,
    _: &str,
    query: &crate::core::checks_logs::LogQuery,
  ) -> Result<Vec<crate::core::checks_logs::LogRecordView>, String> {
    self.record(format!(
      "read check logs {:?} {:?} {:?}",
      query.levels, query.search, query.workflows
    ));
    Ok(self.logs.clone())
  }

  fn log_timeseries(
    &self,
    _: &str,
    _: &crate::core::checks_logs::LogQuery,
  ) -> Result<Vec<crate::core::checks_logs::LogBucket>, String> {
    Ok(self.buckets.clone())
  }

  fn app_setup_logs(
    &self,
    _: &crate::core::checks_logs::LogQuery,
  ) -> Result<Vec<crate::core::checks_logs::LogRecordView>, String> {
    self.record("read app setup logs".into());
    Ok(Vec::new())
  }

  fn system_logs(
    &self,
    query: &crate::core::checks_logs::LogQuery,
  ) -> Result<Vec<crate::core::checks_logs::LogRecordView>, String> {
    self.record(format!("read system logs {:?}", query.search));
    Ok(self.logs.clone())
  }

  fn review_history(
    &self,
    _: &str,
    query: &crate::core::checks_logs::LogQuery,
  ) -> Result<Vec<crate::core::checks_logs::LogRecordView>, String> {
    self.record(format!("read review history {:?}", query.search));
    Ok(self.logs.clone())
  }

  fn open_system_log_dir(&self) -> Result<(), String> {
    self.record("open log folder".into());
    Ok(())
  }

  fn workflows(&self, _: &str) -> Result<Vec<crate::core::checks::WorkflowInfo>, String> {
    Ok(self.workflows.clone())
  }

  fn agent_chats(&self, _: &str) -> Result<Vec<crate::core::agent_chat::AgentChatSummary>, String> {
    Ok(
      self
        .chats
        .iter()
        .map(|c| crate::core::agent_chat::AgentChatSummary {
          session_id: c.session_id,
          pty_session_id: c.pty_session_id.clone(),
          name: c.name.clone(),
          agent: "claude".into(),
          workspace_id: c.workspace_id,
          created_at: c.created_at.clone(),
          message_count: c.messages.len(),
        })
        .collect(),
    )
  }

  fn agent_chat(
    &self,
    _: &str,
    session_id: i64,
  ) -> Result<Option<crate::core::agent_chat::AgentChat>, String> {
    self.record(format!("read agent chat {session_id}"));
    Ok(
      self
        .chats
        .iter()
        .find(|c| c.session_id == session_id)
        .cloned(),
    )
  }

  fn run_sql(&self, _: &str, sql: &str) -> Result<crate::core::checks_logs::SqlResult, String> {
    self.record(format!(
      "run sql {}",
      sql.lines().next().unwrap_or_default()
    ));
    self
      .sql
      .clone()
      .unwrap_or_else(|| Err("no such table".into()))
  }
}

impl commits_data::CommitsHost for FakeHost {
  fn log(
    &self,
    _: &str,
    workspace_id: Option<i64>,
    target_limit: Option<usize>,
    home_limit: Option<usize>,
  ) -> Result<crate::jj::JjLogResult, String> {
    self.record(format!(
      "read log {workspace_id:?} {target_limit:?} {home_limit:?}"
    ));
    self.history.clone().ok_or_else(|| "no log".into())
  }

  fn commit_diff(
    &self,
    _: &str,
    _: Option<i64>,
    revision: &str,
  ) -> Result<crate::jj::JjRevisionDiff, String> {
    self.record(format!("read diff {revision}"));
    self.diff.clone().ok_or_else(|| "no diff".into())
  }

  fn commit_file_diff(
    &self,
    _: &str,
    _: Option<i64>,
    revision: &str,
    file_path: &str,
  ) -> Result<crate::jj::JjFileDiff, String> {
    self.record(format!("read file diff {revision} {file_path}"));
    self
      .diff
      .as_ref()
      .and_then(|d| {
        d.hunks_by_file
          .iter()
          .find(|f| f.path == file_path)
          .cloned()
      })
      .or_else(|| {
        self
          .file_diffs
          .iter()
          .find(|f| f.path == file_path)
          .cloned()
      })
      .ok_or_else(|| "no file diff".into())
  }

  fn abandon(&self, _: &str, _: Option<i64>, change_id: &str) -> Result<String, String> {
    self.record(format!("abandon {change_id}"));
    if self.failing.contains(&"abandon") {
      return Err("abandon failed".into());
    }
    Ok("op1".into())
  }

  fn undo_operation(&self, _: &str, _: Option<i64>, operation_id: &str) -> Result<(), String> {
    self.record(format!("undo operation {operation_id}"));
    Ok(())
  }

  fn stash(&self, _: &str, _: Option<i64>, change_id: &str) -> Result<String, String> {
    self.record(format!("stash {change_id}"));
    Ok("st1".into())
  }

  fn undo_commit(&self, _: &str, _: i64, change_id: &str) -> Result<(), String> {
    self.record(format!("undo commit {change_id}"));
    Ok(())
  }

  fn revert(&self, _: &str, _: Option<i64>, change_id: &str) -> Result<(), String> {
    self.record(format!("revert {change_id}"));
    Ok(())
  }

  fn shift_to_now(&self, _: &str, workspace_id: i64) -> Result<(), String> {
    self.record(format!("shift to now {workspace_id}"));
    Ok(())
  }

  fn commit_description(&self, _: &str, _: Option<i64>, change_id: &str) -> Result<String, String> {
    self.record(format!("read description {change_id}"));
    Ok(self.description.clone())
  }

  fn describe(
    &self,
    _: &str,
    _: Option<i64>,
    change_id: &str,
    description: &str,
  ) -> Result<(), String> {
    self.record(format!("describe {change_id} {description:?}"));
    Ok(())
  }

  fn shift_timestamp(
    &self,
    _: &str,
    _: i64,
    change_id: &str,
    shift: &commits_data::TimeShift,
  ) -> Result<(), String> {
    self.record(format!("shift {change_id} {shift:?}"));
    Ok(())
  }

  fn start_resolve(
    &self,
    _: &str,
    workspace_id: Option<i64>,
    change_ids: Option<Vec<String>>,
  ) -> Result<crate::core::ResolveConflictsSession, String> {
    self.record(format!("start resolve {workspace_id:?} {change_ids:?}"));
    if self.failing.contains(&"start_resolve") {
      return Err("No conflicted commits found to resolve.".into());
    }
    serde_json::from_value(serde_json::json!({
      "targets": change_ids.unwrap_or_default().iter().map(|c| serde_json::json!({
        "change_id": c, "commit_id": c, "description": "", "conflicted_files": [],
        "resolve_path": "/r", "workspace_id": 4, "workspace_name": "w"
      })).collect::<Vec<_>>(),
      "agent_cwd": "/r/cwd", "source_workspace_id": workspace_id, "workspace_slug": "w"
    }))
    .map_err(|e| e.to_string())
  }

  fn resolve_prompt(&self, prompt: &str, _: &crate::core::ResolveConflictsSession) -> String {
    format!("resolve: {prompt}")
  }

  fn snapshot_working_copy(&self, path: &str) -> Result<String, String> {
    self.record(format!("snapshot {path}"));
    Ok("snap1".into())
  }

  fn discard_all(&self, path: &str) -> Result<(), String> {
    self.record(format!("discard {path}"));
    Ok(())
  }

  fn restore_snapshot(&self, path: &str, snapshot_id: &str) -> Result<(), String> {
    self.record(format!("restore {path} {snapshot_id}"));
    Ok(())
  }
}

impl modal::ModalsHost for FakeHost {
  fn prompt_history(&self, _: &str) -> Result<Vec<crate::local_db::PromptHistoryEntry>, String> {
    self.record("read prompt history".into());
    Ok(self.prompts.clone())
  }

  fn stashes(&self, _: &str) -> Result<Vec<crate::local_db::StashEntry>, String> {
    self.record("read stashes".into());
    Ok(self.stash_list.lock().unwrap().clone())
  }

  fn stash_diff(&self, _: &str, id: i64) -> Result<crate::jj::JjRevisionDiff, String> {
    self.record(format!("read stash diff {id}"));
    self.stash_diff.clone().ok_or_else(|| "no diff".into())
  }

  fn apply_stash(&self, _: &str, id: i64, target: &str) -> Result<(), String> {
    self.record(format!("apply stash {id} {target}"));
    self.apply_error.clone().map_or(Ok(()), Err)
  }

  fn delete_stash(&self, _: &str, id: i64) -> Result<(), String> {
    self.record(format!("delete stash {id}"));
    self.stash_list.lock().unwrap().retain(|s| s.id != id);
    Ok(())
  }

  fn stash_patch(&self, _: &str, id: i64) -> Result<String, String> {
    self.record(format!("export stash {id}"));
    Ok(format!("patch of stash {id}"))
  }
}

impl checklist::ToolsHost for FakeHost {
  fn check_prerequisites(&self) -> Vec<crate::core::PrerequisiteStatus> {
    self.record("check".into());
    self
      .installed
      .iter()
      .map(|(binary, installed)| crate::core::PrerequisiteStatus {
        binary: binary.to_string(),
        installed: *installed,
      })
      .collect()
  }

  fn open_url(&self, url: &str) {
    self.record(format!("open {url}"));
  }
}

impl account_data::AccountHost for FakeHost {
  fn sign_out(&self) {
    self.record("sign out".into());
  }

  fn exchange_token(&self, token: &str) -> Result<(), String> {
    self.record(format!("exchange {token}"));
    if token == "expired" {
      return Err("Token exchange failed".into());
    }
    Ok(())
  }

  fn github_repositories(&self) -> Result<Vec<crate::core::account::GithubRepository>, String> {
    self.record("read github repositories".into());
    self.github_repos.clone().unwrap_or_else(|| Ok(Vec::new()))
  }

  fn linear_oauth_url(&self) -> Result<String, crate::core::supabase::ApiError> {
    self.record("linear oauth".into());
    self
      .oauth
      .clone()
      .unwrap_or_else(|| Ok("https://linear.app/oauth/authorize".into()))
  }

  fn delete_account(&self) -> Result<(), crate::core::supabase::ApiError> {
    self.record("delete account".into());
    self.account_deletion.clone().map_or(Ok(()), Err)
  }

  fn cloud_action(&self, action: account_data::CloudAction) {
    match action {
      account_data::CloudAction::Refresh => self.record("read cloud workspace".into()),
      action => self.record(format!("cloud {action:?}")),
    }
  }
}

impl remote_data::RemoteHost for FakeHost {
  fn ssh_aliases(&self) -> Vec<String> {
    vec!["devbox".into(), "staging".into()]
  }

  fn ssh_identities(&self) -> Vec<crate::core::remote_local_keys::LocalSshIdentity> {
    self.ssh_keys.clone()
  }

  fn probe_host_key(&self, hostname: &str, port: u16) -> Result<String, String> {
    self.record(format!("probe {hostname}:{port}"));
    self
      .host_key
      .clone()
      .unwrap_or_else(|| Ok("SHA256:abc123".into()))
  }

  fn resolve_alias(
    &self,
    alias: &str,
  ) -> Result<crate::core::remote_ssh_config::ResolvedSshAlias, String> {
    self.record(format!("resolve {alias}"));
    self
      .resolved_alias
      .clone()
      .unwrap_or_else(|| Err("unknown alias".into()))
  }

  fn persistent_sessions(
    &self,
    endpoint: &crate::core::remote_control_plane::SshEndpoint,
    repo: &str,
    workspace: &str,
  ) -> Result<Vec<crate::core::pty_remote_supervisor::PtySessionInfo>, String> {
    self.record(format!("sessions {}:{repo}:{workspace}", endpoint.hostname));
    self
      .remote_sessions
      .clone()
      .unwrap_or_else(|| Ok(Vec::new()))
  }
}

/// The window's root over the screen, as the shell embeds it: cached, so a
/// test sees what a frame the screen did not ask for would show.
struct Framed {
  screen: Entity<Screen>,
}

impl Render for Framed {
  fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
    div().size_full().child(Screen::element(&self.screen))
  }
}

impl Harness {
  fn new(cx: &mut TestAppContext, snapshot: ScreenSnapshot) -> Self {
    Self::with_host(cx, snapshot, Arc::new(FakeHost::default()))
  }

  fn with_host(cx: &mut TestAppContext, snapshot: ScreenSnapshot, host: Arc<FakeHost>) -> Self {
    let sent = Rc::new(RefCell::new(Vec::new()));
    let sink: ActionSink = {
      let sent = sent.clone();
      Rc::new(move |action, _| sent.borrow_mut().push(action))
    };
    cx.update(crate::gpui_shell::theme::init);
    let shown = Rc::new(RefCell::new(Vec::new()));
    let popups: crate::gpui_shell::popup::PopupSink = {
      let shown = shown.clone();
      Rc::new(move |request, _| shown.borrow_mut().push(request))
    };
    let (root, cx) = cx.add_window_view(|_, cx| Framed {
      screen: cx.new(|cx| Screen::new(snapshot, sink, popups, host, cx)),
    });
    let screen = root.read_with(cx, |root, _| root.screen.clone());
    let cx = cx.clone();
    Self {
      cx,
      screen,
      sent,
      popups: shown,
    }
  }

  /// Picks `text` in the menu shown last, as a click on it would.
  fn pick(&mut self, text: &str) {
    use crate::gpui_shell::popup::{MenuEntry, PopupRequest};
    self.cx.run_until_parked();
    let entries = match self.popups.borrow().last() {
      Some(
        PopupRequest::ShowDropdown { entries, .. }
        | PopupRequest::ShowSelect { entries, .. }
        | PopupRequest::ShowMenu { entries, .. },
      ) => entries.clone(),
      _ => panic!("no menu is shown"),
    };
    let id = entries
      .iter()
      .find_map(|e| match e {
        MenuEntry::Item { id, text: t, .. } if t == text => Some(*id),
        _ => None,
      })
      .unwrap_or_else(|| panic!("{text} is not in the menu"));
    self
      .cx
      .update(|_, cx| crate::gpui_shell::popup::pick_native(id, cx));
    self.cx.run_until_parked();
  }

  fn drawn(&mut self, selector: &'static str) -> bool {
    self.cx.run_until_parked();
    self.cx.debug_bounds(selector).is_some()
  }

  /// Clicks `button` in the confirmation on screen, found by role and
  /// name, as the user would.
  fn answer(&mut self, button: &str) {
    use gpui_testing_library::prelude::*;
    assert!(
      self.drawn(crate::gpui_shell::confirm::ID),
      "no confirmation"
    );
    let window = self.cx.update(|window, _| window.window_handle());
    let screen = gpui_testing_library::Screen::new(&self.cx, window);
    screen.redraw();
    // The row behind it may hold a button of the same name.
    let panel = gpui::ElementId::from(format!("{}-panel", crate::gpui_shell::confirm::ID));
    let target = screen
      .get_all_by_role(by::role(gpui::Role::Button).name(button))
      .into_iter()
      .find(|b| b.path().contains(&panel))
      .unwrap_or_else(|| panic!("no {button} button in the confirmation"));
    screen.user().click(&target);
    self.cx.run_until_parked();
  }

  fn click(&mut self, selector: &'static str) {
    self.cx.run_until_parked();
    let bounds = self
      .cx
      .debug_bounds(selector)
      .unwrap_or_else(|| panic!("{selector} is not drawn"));
    self.cx.simulate_click(bounds.center(), Modifiers::none());
  }

  fn right_click(&mut self, selector: &'static str) {
    self.cx.run_until_parked();
    let at = self
      .cx
      .debug_bounds(selector)
      .unwrap_or_else(|| panic!("{selector} is not drawn"))
      .center();
    self
      .cx
      .simulate_mouse_down(at, gpui::MouseButton::Right, Modifiers::none());
    self
      .cx
      .simulate_mouse_up(at, gpui::MouseButton::Right, Modifiers::none());
  }

  fn sent(&self) -> Vec<ScreenAction> {
    self.sent.borrow().clone()
  }

  /// Hands the screens Rust's account, as `gpui_shell::account_changed`.
  fn set_account(&mut self, account: serde_json::Value) {
    let account: settings::AccountRoute = serde_json::from_value(account).expect("account");
    self
      .cx
      .update(|_, cx| cx.set_global(account_data::Account(Some(account))));
    self.cx.run_until_parked();
  }

  fn update(&mut self, f: impl FnOnce(&mut ScreenSnapshot)) {
    let mut snapshot = self.screen.read_with(&self.cx, |s, _| s.snapshot().clone());
    f(&mut snapshot);
    self
      .screen
      .update(&mut self.cx, |s, cx| s.set_snapshot(snapshot, cx));
  }
}

fn onboarding() -> ScreenSnapshot {
  serde_json::from_str(r#"{"kind": "onboarding", "dark": false}"#).expect("parse")
}

/// This computer with `/home/me/app`, an SSH host with `/srv/app` saved
/// on it, and a stopped dev container.
fn listing() -> machines::Listing {
  use machines::{Machine, Place, Recent};
  machines::Listing {
    machines: vec![
      Machine {
        id: "local".into(),
        place: Place::Local,
        name: "This computer".into(),
        detail: "laptop".into(),
        terminals: None,
      },
      Machine {
        id: "ssh:ep-1".into(),
        place: Place::Ssh {
          endpoint_id: "ep-1".into(),
        },
        name: "Dev box".into(),
        detail: "me@devbox".into(),
        terminals: Some(Default::default()),
      },
      Machine {
        id: "cloud".into(),
        place: Place::Cloud {
          endpoint_id: None,
          status: machines::CLOUD_NOT_CREATED.into(),
        },
        name: "Cloud workspace".into(),
        detail: "Not created yet".into(),
        terminals: None,
      },
      Machine {
        id: "devcontainer:c1".into(),
        place: Place::DevContainer {
          container_id: "c1".into(),
          repo_path: "/home/me/web".into(),
          running: false,
        },
        name: "web".into(),
        detail: "Stopped".into(),
        terminals: None,
      },
    ],
    recents: vec![
      Recent {
        machine_id: "local".into(),
        name: "app".into(),
        path: "/home/me/app".into(),
        saved_id: None,
      },
      Recent {
        machine_id: "ssh:ep-1".into(),
        name: "app".into(),
        path: "/srv/app".into(),
        saved_id: Some("r1".into()),
      },
    ],
    signed_out: false,
    ssh: false,
    containers: false,
  }
}

fn running_daemon(sessions: usize) -> crate::core::pty_remote_supervisor::TerminalStatus {
  crate::core::pty_remote_supervisor::TerminalStatus {
    daemon: Some(crate::core::pty_remote_supervisor::DaemonStatus {
      pid: 4242,
      version: "0.3.0".into(),
      sessions,
    }),
    tmux_installed: true,
    tmux_sessions: 0,
  }
}

fn settings_route(tab: &str) -> ScreenSnapshot {
  serde_json::from_value(serde_json::json!({
    "kind": "settings",
    "dark": false,
    "tab": tab,
    "repoPath": "/repo",
    "currentBranch": "main",
  }))
  .expect("parse")
}

fn settings() -> ScreenSnapshot {
  settings_route("repository")
}

/// The settings screen as last drawn.
fn drawn_settings(h: &mut Harness) -> settings::Settings {
  h.cx.run_until_parked();
  h.screen.read_with(&h.cx, |screen, cx| {
    screen
      .settings
      .as_ref()
      .expect("settings drawn")
      .read(cx)
      .screen()
  })
}

fn route_json(workspace_id: Option<i64>, home_branch: &str) -> serde_json::Value {
  serde_json::json!({
    "kind": "workspace",
    "dark": false,
    "repoPath": "/repo",
    "workspaceId": workspace_id,
    "homeBranch": home_branch,
    "tab": "overview",
    "retargeting": false,
    "creatingPr": false,
    "runningChecks": [],
    "revision": 0
  })
}

fn workspace_route() -> ScreenSnapshot {
  serde_json::from_value(route_json(Some(7), "main")).expect("workspace route")
}

fn route_of(snapshot: &ScreenSnapshot) -> &workspace_data::Route {
  match &snapshot.view {
    View::Workspace(route) => route,
    _ => unreachable!(),
  }
}

fn workspace_row(branch: &str) -> serde_json::Value {
  serde_json::json!({
    "id": 7,
    "repo_path": "/repo",
    "workspace_name": "login",
    "workspace_path": "/repo/.treq/workspaces/login",
    "branch_name": branch,
    "created_at": "2026-10-19T00:00:00Z",
    "refreshed_at": null,
    "metadata": r#"{"linear_issue_key":"ENG-12","linear_issue_url":"https://linear.app/t/ENG-12","linear_issue_title":"Login"}"#,
    "target_branch": "main",
    "title": "Add login",
    "description": null,
    "moved_files": null,
    "not_on_remote": false,
    "sparse_patterns": null
  })
}

/// A feature workspace two commits ahead of its remote, with an open pull
/// request, as the header in `WORKSPACE_JSON` shows.
fn feature_loaded() -> workspace_data::Loaded {
  let row = workspace_row("feature/login");
  let ahead = serde_json::json!({"hash": "a", "timestamp": "t", "message": "m"});
  let status = serde_json::json!({
    "current": row,
    "has_conflicts": false,
    "has_changes": true,
    "commits_ahead": 3,
    "conflicted_files": [],
    "remote_sync": {"type": "Ahead", "data": {"count": 2}},
    "target": null,
    "children": [],
    "dag_nodes": [],
    "conflicted_workspace_ids": [],
    "commits_ahead_of_target": [ahead, ahead, ahead]
  });
  let commits = serde_json::json!({
    "commits": [commit("a", 10, 4), commit("b", 2, 0)],
    "target_branch": "main",
    "workspace_branch": "feature/login"
  });
  workspace_data::Loaded {
    workspace: Some(serde_json::from_value(row).expect("workspace")),
    status: Some(serde_json::from_value(status).expect("status")),
    remote: Some(crate::github::GitRemoteInfo {
      owner: "tznc".into(),
      repo: "treq".into(),
      full_name: "tznc/treq".into(),
    }),
    commits: Some(serde_json::from_value(commits).expect("commits")),
    review_count: 5,
    uncommitted_count: 2,
    checks: Some(crate::gpui_shell::popover::data::ChecksData {
      workflows: ["a.yml", "b.yml", "c.yml"]
        .into_iter()
        .enumerate()
        .map(|(i, f)| crate::gpui_shell::popover::data::Workflow {
          filename: f.into(),
          name: f.into(),
          latest: Some((i as i64 + 1, "passed".into())),
        })
        .collect(),
      trusted: true,
      setup: None,
    }),
    linear_enabled: true,
    ..Default::default()
  }
}

fn open_pr() -> workspace_data::PrCache {
  workspace_data::PrCache {
    pr: Some(Some(crate::github::PrInfo {
      number: 42,
      title: "Add login".into(),
      state: "OPEN".into(),
      url: "https://github.com/tznc/treq/pull/42".into(),
      head_ref_name: "feature/login".into(),
      base_ref_name: "main".into(),
      merge_state_status: None,
      is_draft: false,
      body: String::new(),
    })),
    ci: Some(crate::github::PrCiStatus {
      state: "success".into(),
      total: 4,
      passed: 4,
      failed: 0,
      pending: 0,
      checks: vec![],
    }),
  }
}

fn feature_host() -> FakeHost {
  FakeHost {
    loaded: feature_loaded(),
    pr: open_pr(),
    ..FakeHost::default()
  }
}

/// The workspace screen in a window as wide as the app's default.
fn workspace_harness(
  cx: &mut TestAppContext,
  host: FakeHost,
  snapshot: ScreenSnapshot,
) -> (Harness, Arc<FakeHost>) {
  let host = Arc::new(host);
  let h = Harness::with_host(cx, snapshot, host.clone());
  h.cx
    .simulate_resize(gpui::size(gpui::px(1400.), gpui::px(900.)));
  (h, host)
}

fn settings_act(h: &mut Harness, action: ScreenAction) {
  let view = settings_view(h);
  view.update(&mut h.cx, |view, cx| view.handle(action, cx));
  h.cx.run_until_parked();
}

/// Rust's account, signed out, with `patch` merged in.
fn account_state(patch: serde_json::Value) -> serde_json::Value {
  let mut state = serde_json::json!({
    "loading": false,
    "signingIn": false,
    "signInError": null,
    "user": null,
    "subscription": null,
    "devCallback": { "loading": false, "error": null },
    "cloud": null,
    "github": null,
    "linearOauthLoading": false,
  });
  if let (Some(state), Some(patch)) = (state.as_object_mut(), patch.as_object()) {
    for (key, value) in patch {
      state.insert(key.clone(), value.clone());
    }
  }
  state
}

fn ada() -> serde_json::Value {
  serde_json::json!({ "name": "Ada", "email": "ada@example.com", "avatarUrl": null })
}

fn signed_in_as_ada(cx: &mut TestAppContext, host: Arc<FakeHost>) -> Harness {
  let mut h = Harness::with_host(cx, account_route("account"), host);
  h.set_account(account_state(
    serde_json::json!({ "user": ada(), "subscription": pro() }),
  ));
  h
}

fn merge_preview(commits: usize) -> merge::Preview {
  let commit = |i: usize| {
    serde_json::json!({
      "commit_id": format!("c{i}"),
      "short_id": format!("c{i}"),
      "change_id": format!("k{i}"),
      "description": format!("Change {i}"),
      "author_name": "Zn",
      "timestamp": "2026-10-19T00:00:00Z",
      "parent_ids": [],
      "is_working_copy": false,
      "bookmarks": [],
      "is_immutable": false,
      "insertions": 3,
      "deletions": 1,
    })
  };
  merge::Preview {
    workspace: Some(merge_workspace(Some(r#"{"description": "Adds a thing"}"#))),
    commits: Some(
      serde_json::from_value(serde_json::json!({
        "commits": (0..commits).map(commit).collect::<Vec<_>>(),
        "total_count": commits,
      }))
      .expect("commits"),
    ),
    diff: Some(
      serde_json::from_value(serde_json::json!({
        "committed_files": [{
          "path": "src/a.rs",
          "status": "M",
          "previous_path": null,
          "changed_line_count": 2,
          "diff_deferred": false,
        }],
        "hunks_by_file": [{
          "path": "src/a.rs",
          "hunks": [{
            "id": "h0",
            "header": "@@ -1 +1 @@",
            "lines": ["-old", "+new"],
            "patch": "",
          }],
        }],
        "too_large_to_render": false,
      }))
      .expect("diff"),
    ),
    plan: Some(merge_plan(&[], false)),
    ..merge::Preview::default()
  }
}

/// The landing plan of the `feature` workspace into `develop`, moving
/// `retargeted` children onto it.
fn merge_plan(retargeted: &[&str], can_rollup: bool) -> crate::core::MergeLandingPlan {
  crate::core::MergeLandingPlan {
    target_branch: "develop".into(),
    landed_branches: vec!["feature".into()],
    retargeted: retargeted
      .iter()
      .enumerate()
      .map(|(i, branch)| crate::core::WorkspaceRetarget {
        workspace_id: 20 + i as i64,
        branch_name: branch.to_string(),
        from_branch: "feature".into(),
        to_branch: "develop".into(),
      })
      .collect(),
    can_rollup,
  }
}

fn merge_harness(cx: &mut TestAppContext, host: FakeHost) -> (Harness, Arc<FakeHost>) {
  let host = Arc::new(host);
  let mut h = Harness::with_host(cx, merge_route(), host.clone());
  assert!(h.drawn("merge-confirm"));
  (h, host)
}

/// The feature workspace's Code tab, with two files and two commits.
fn code_host() -> FakeHost {
  let mut loaded = feature_loaded();
  if let Some(ws) = loaded.workspace.as_mut() {
    ws.description = Some("Fix the login flow".into());
  }
  let entry = |name: &str, is_directory: bool| crate::core::WorkspaceEntry {
    name: name.into(),
    path: format!("/repo/.treq/workspaces/login/{name}"),
    is_directory,
    modified_at: None,
    submodule_pin: None,
    submodule_synced: None,
    status: None,
  };
  FakeHost {
    loaded,
    pr: open_pr(),
    overview: code_data::Overview {
      entries: vec![entry("src", true), entry("README.md", false)],
      readme: Some("# Login".into()),
      unavailable: false,
    },
    history: serde_json::from_value(serde_json::json!({
      "commits": [commit("a", 10, 4), commit("b", 2, 0)],
      "target_branch": "main",
      "workspace_branch": "feature/login"
    }))
    .ok(),
    files: vec!["src/login.rs".into(), "src/logout.rs".into()],
    ..FakeHost::default()
  }
}

fn code_route() -> ScreenSnapshot {
  let mut route = route_json(Some(7), "main");
  route["code"] = true.into();
  route["frameHeight"] = 120.into();
  serde_json::from_value(route).expect("code route")
}

fn fields(h: &Harness, name: &str) -> Vec<serde_json::Value> {
  h.sent()
    .into_iter()
    .filter_map(|a| match a {
      ScreenAction::Field { field, value } if field == name => Some(value),
      _ => None,
    })
    .collect()
}

fn commits_host() -> FakeHost {
  let mut host = code_host();
  host.diff = serde_json::from_value(serde_json::json!({
    "committed_files": [{
      "path": "src/a.rs", "status": "M", "previous_path": null,
      "changed_line_count": 2, "diff_deferred": false
    }],
    "hunks_by_file": [{
      "path": "src/a.rs",
      "hunks": [{"id": "h", "header": "@@ -1,2 +1,3 @@", "lines": [" a", "+b", "+c"], "patch": ""}]
    }],
    "too_large_to_render": false
  }))
  .ok();
  host.description = "work".into();
  host
}

fn commits_route() -> ScreenSnapshot {
  let mut route = route_json(Some(7), "main");
  route["code"] = true.into();
  route["tab"] = "commits".into();
  serde_json::from_value(route).expect("commits route")
}

fn changes_host() -> FakeHost {
  let mut host = code_host();
  host.working = serde_json::from_value(serde_json::json!({
    "committed_files": [],
    "uncommitted_files": [
      {"path": "a.rs", "status": "M", "previous_path": null, "changed_line_count": 2, "diff_deferred": false},
      {"path": "b.rs", "status": "A", "previous_path": null, "changed_line_count": 1, "diff_deferred": false}
    ],
    "hunks_by_file": [
      {"path": "a.rs", "hunks": [{"id": "h", "header": "@@ -1,2 +1,3 @@", "lines": [" a", "+b", "+c"], "patch": ""}]},
      {"path": "b.rs", "hunks": [{"id": "h", "header": "@@ -0,0 +1,1 @@", "lines": ["+d"], "patch": ""}]}
    ],
    "too_large_to_render": false
  }))
  .ok();
  host
}

fn changes_route() -> ScreenSnapshot {
  let mut route = route_json(Some(7), "main");
  route["code"] = true.into();
  route["tab"] = "changes".into();
  serde_json::from_value(route).expect("changes route")
}

fn bump_changes_revision(h: &mut Harness) {
  h.update(|s| {
    if let View::Workspace(r) = &mut s.view {
      r.changes_revision += 1;
    }
  });
  h.cx.run_until_parked();
}

fn count_calls(host: &FakeHost, prefix: &str) -> usize {
  host
    .calls()
    .iter()
    .filter(|c| c.starts_with(prefix))
    .count()
}

/// The selected lines of the Changes tab: the file, and each line.
fn selected_lines(h: &mut Harness) -> Option<(String, Vec<(usize, usize)>)> {
  h.cx.run_until_parked();
  h.screen.read_with(&h.cx, |screen, cx| {
    let workspace = screen.workspace.as_ref().expect("workspace").read(cx);
    let changes = workspace.changes().expect("changes").read(cx);
    changes
      .review
      .selection
      .as_ref()
      .map(|s| (s.path.clone(), s.lines.clone()))
  })
}

/// Comments line `+b` of a.rs with `text`.
fn comment_line(h: &mut Harness, text: &str) {
  h.click("add-comment-a.rs-0-1");
  assert!(h.drawn("comment-input"));
  h.click("comment-input");
  h.cx.simulate_input(text);
  h.click("comment-submit");
  h.cx.run_until_parked();
}

fn settle_save(h: &mut Harness) {
  h.cx
    .executor()
    .advance_clock(std::time::Duration::from_millis(500));
  h.cx.run_until_parked();
}

fn gh_thread(id: &str, line: i64, resolved: bool) -> crate::github::GhReviewThread {
  crate::github::GhReviewThread {
    id: id.into(),
    is_resolved: resolved,
    is_outdated: false,
    path: "a.rs".into(),
    line: Some(line),
    diff_side: "RIGHT".into(),
    comments: vec![crate::github::GhReviewComment {
      id: format!("{id}-c"),
      body: "Why two lines?".into(),
      author: crate::github::GhAuthor {
        login: "octo".into(),
        avatar_url: None,
      },
      created_at: "2026-03-04T00:00:00Z".into(),
      diff_hunk: String::new(),
      url: "https://github.com/o/r/pull/1#c".into(),
      reactions: Vec::new(),
    }],
  }
}

fn log_record(body: &str, severity: &str) -> crate::core::checks_logs::LogRecordView {
  crate::core::checks_logs::LogRecordView {
    timestamp: "2026-10-19T10:00:01.250Z".into(),
    severity_number: 9,
    severity_text: severity.into(),
    body: body.into(),
    trace_id: String::new(),
    span_id: String::new(),
    run_id: 3,
    job_id: "build".into(),
    step_index: 0,
    step_name: "Test".into(),
    stream: "stdout".into(),
  }
}

fn logs_host() -> FakeHost {
  let mut host = code_host();
  host.logs = vec![
    log_record("compiling", "INFO"),
    log_record("test failed", "ERROR"),
  ];
  host
}

fn logs_route(source: Option<&str>) -> ScreenSnapshot {
  let mut route = route_json(Some(7), "main");
  route["code"] = true.into();
  route["tab"] = "logs".into();
  route["logsSource"] = source.into();
  serde_json::from_value(route).expect("logs route")
}

fn terminal_host() -> FakeHost {
  let mut host = code_host();
  host.sessions = vec![crate::local_db::Session {
    id: 3,
    workspace_id: Some(7),
    name: "Claude 1".into(),
    created_at: String::new(),
    last_accessed: String::new(),
    model: None,
  }];
  host.workspaces = host.loaded.workspace.clone().into_iter().collect();
  host
}

fn terminal_route(terminal: serde_json::Value) -> ScreenSnapshot {
  let mut route = route_json(Some(7), "main");
  route["code"] = true.into();
  route["terminal"] = terminal;
  serde_json::from_value(route).expect("terminal route")
}

fn set_terminal(h: &mut Harness, terminal: serde_json::Value) {
  h.update(|s| {
    if let View::Workspace(route) = &mut s.view {
      route.terminal = Some(serde_json::from_value(terminal).expect("terminal"));
    }
  });
  h.cx.run_until_parked();
}

fn agent_pty() -> String {
  terminal_data::agent_pty_id("/repo", 3)
}

fn pty_writes(host: &FakeHost) -> Vec<String> {
  host
    .calls()
    .into_iter()
    .filter(|c| c.starts_with("write "))
    .collect()
}

fn prompt_entry(
  id: i64,
  workspace_id: Option<i64>,
  label: Option<&str>,
  text: &str,
  agent: Option<&str>,
) -> crate::local_db::PromptHistoryEntry {
  crate::local_db::PromptHistoryEntry {
    id,
    workspace_id,
    session_id: None,
    prompt_text: text.into(),
    agent: agent.map(Into::into),
    created_at: format!("2026-10-19T1{id}:05:09Z"),
    workspace_label: label.map(Into::into),
  }
}

/// A screen with `modal` drawn over it.
fn modal_harness(
  cx: &mut TestAppContext,
  host: FakeHost,
  modal: serde_json::Value,
) -> (Harness, Arc<FakeHost>) {
  let (mut h, host) = workspace_harness(cx, host, onboarding());
  h.set_modal(Some(modal));
  (h, host)
}

fn history_modal() -> serde_json::Value {
  serde_json::json!({ "kind": "promptHistory", "repoPath": "/repo" })
}

fn stash_modal() -> serde_json::Value {
  serde_json::json!({
    "kind": "stash",
    "repoPath": "/repo",
    "workspaces": [
      { "branch": "treq/login", "label": "Login fixes" },
      { "branch": "treq/docs", "label": "treq/docs" },
    ],
  })
}

fn stash_host() -> FakeHost {
  FakeHost {
    stash_list: Mutex::new(vec![
      stash_entry(1, "login", &["src/login.rs"]),
      stash_entry(2, "docs", &["a.md", "b.md"]),
    ]),
    stash_diff: Some(stash_diff()),
    ..FakeHost::default()
  }
}

fn agent_modal() -> serde_json::Value {
  serde_json::json!({
    "kind": "agentPrompt",
    "repoPath": "/repo",
    "defaultBranch": "main",
    "workspaces": [
      { "id": 7, "branchName": "treq/login", "path": "/repo/.treq/workspaces/login" },
      { "id": 8, "branchName": "main", "path": "/repo" },
    ],
    "initialPrompt": "fix it",
    "initialWorkspaceId": 7,
  })
}

/// The prompt dialog's starts the backend was asked for.
fn tasks(h: &mut Harness, host: &FakeHost) -> Vec<launch_data::TaskRequest> {
  h.cx.run_until_parked();
  host.tasks.lock().unwrap().clone()
}

fn setup_route(allow_user_managed: bool, cloud: serde_json::Value) -> serde_json::Value {
  assert_eq!(
    cloud,
    serde_json::json!({}),
    "the cloud workspace is Rust's account"
  );
  serde_json::json!({
    "setup": { "allowUserManaged": allow_user_managed }
  })
}

/// The remote dialogs, signed in as Ada.
fn remote_harness(
  cx: &mut TestAppContext,
  host: FakeHost,
  remote: serde_json::Value,
) -> (Harness, Arc<FakeHost>) {
  let (mut h, host) = workspace_harness(cx, host, remote_snapshot(remote));
  h.set_account(account_state(serde_json::json!({ "user": ada() })));
  (h, host)
}

fn settings_view(h: &mut Harness) -> Entity<settings::SettingsView> {
  h.cx.run_until_parked();
  h.screen
    .read_with(&h.cx, |screen, _| screen.settings.clone())
    .expect("settings drawn")
}

fn pro() -> serde_json::Value {
  serde_json::json!({ "plan": "pro", "status": "active", "currentPeriodEnd": "2026-11-01T12:00:00Z" })
}

fn account_route(tab: &str) -> ScreenSnapshot {
  serde_json::from_value(serde_json::json!({
    "kind": "settings",
    "dark": false,
    "tab": tab,
    "repoPath": "/repo",
    "currentBranch": "main",
  }))
  .expect("parse")
}

fn merge_route() -> ScreenSnapshot {
  serde_json::from_value(serde_json::json!({
    "kind": "mergePreview",
    "dark": false,
    "repoPath": "/repo",
    "workspaceId": 7,
  }))
  .expect("parse")
}

fn merge_workspace(metadata: Option<&str>) -> crate::local_db::Workspace {
  serde_json::from_value(serde_json::json!({
    "id": 7,
    "repo_path": "/repo",
    "workspace_name": "feature",
    "workspace_path": "feature",
    "branch_name": "feature",
    "created_at": "2026-10-19T00:00:00Z",
    "refreshed_at": null,
    "metadata": metadata,
    "target_branch": "develop",
    "title": "Feature",
    "description": null,
    "moved_files": null,
    "not_on_remote": false,
    "sparse_patterns": null,
  }))
  .expect("workspace")
}

fn stash_entry(id: i64, label: &str, files: &[&str]) -> crate::local_db::StashEntry {
  crate::local_db::StashEntry {
    id,
    workspace_id: Some(7),
    workspace_label: label.into(),
    commit_id: format!("commit{id}"),
    change_id: format!("change{id}"),
    short_commit_id: format!("abc{id}"),
    bookmark_name: format!("treq/stash-{id}"),
    created_at: "2026-10-19T13:05:09Z".into(),
    additions: 4,
    deletions: 1,
    files_changed: files.iter().map(|f| f.to_string()).collect(),
  }
}

fn stash_diff() -> crate::jj::JjRevisionDiff {
  serde_json::from_value(serde_json::json!({
    "committed_files": [],
    "hunks_by_file": [{
      "path": "src/login.rs",
      "hunks": [{
        "id": "h1",
        "header": "@@ -1,2 +1,2 @@",
        "lines": [" fn login() {", "-  old();", "+  new();"],
        "patch": "",
      }],
    }],
    "too_large_to_render": false,
  }))
  .expect("diff")
}

fn remote_snapshot(remote: serde_json::Value) -> ScreenSnapshot {
  serde_json::from_value(serde_json::json!({
    "kind": "onboarding",
    "dark": false,
    "remote": remote,
  }))
  .expect("parse")
}

fn field(field: &str, value: serde_json::Value) -> ScreenAction {
  ScreenAction::Field {
    field: field.into(),
    value,
  }
}

fn commit(id: &str, insertions: u32, deletions: u32) -> serde_json::Value {
  serde_json::json!({
    "commit_id": id,
    "short_id": id,
    "change_id": id,
    "description": "work",
    "author_name": "Zn",
    "timestamp": "2026-10-19T00:00:00Z",
    "parent_ids": [],
    "is_working_copy": false,
    "bookmarks": [],
    "is_immutable": false,
    "insertions": insertions,
    "deletions": deletions
  })
}

fn press(id: &str) -> ScreenAction {
  ScreenAction::Press { id: id.into() }
}

fn type_into(h: &mut Harness, selector: &'static str, text: &str) {
  h.click(selector);
  h.cx.simulate_input(text);
}

impl Harness {
  fn set_modal(&mut self, modal: Option<serde_json::Value>) {
    let modal = modal.map(|m| serde_json::from_value(m).expect("modal"));
    self
      .screen
      .update(&mut self.cx, |screen, cx| screen.set_modal(modal, cx));
    self.cx.run_until_parked();
  }

  fn clipboard(&mut self) -> Option<String> {
    self
      .cx
      .update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text()))
  }

  /// A click on the backdrop, outside the dialog.
  fn click_backdrop(&mut self) {
    self.cx.run_until_parked();
    self
      .cx
      .simulate_click(gpui::point(gpui::px(3.), gpui::px(3.)), Modifiers::none());
    self.cx.run_until_parked();
  }
}

impl workspace_dialog_data::WorkspaceDialogHost for FakeHost {
  fn all_workspaces(&self, _: &str) -> Result<Vec<crate::local_db::Workspace>, String> {
    let mut all = self.workspaces.clone();
    all.extend(self.created.lock().unwrap().iter().cloned());
    Ok(all)
  }

  fn commits_ahead(&self, _: &str, id: i64) -> Result<Vec<crate::core::WorkspaceCommit>, String> {
    self.record(format!("read ahead {id}"));
    Ok(self.ahead.clone())
  }

  fn repo_branches(&self, _: &str) -> Result<Vec<crate::jj::JjBranch>, String> {
    self.record("read branches".into());
    Ok(self.branches.clone())
  }

  fn fetch_in_background(&self, _: &str) {
    self.record("fetch".into());
  }

  fn default_branch(&self, _: &str) -> Result<String, String> {
    Ok("main".into())
  }

  fn current_branch(&self, _: &str) -> Result<Option<String>, String> {
    Ok(self.home_branch.clone())
  }

  fn branch_status(&self, _: &str, branch: &str) -> Result<crate::jj::BranchStatus, String> {
    self.record(format!("check branch {branch}"));
    let remote = self.remote_branches.iter().any(|b| b == branch);
    Ok(crate::jj::BranchStatus {
      local_exists: self.local_branches.iter().any(|b| b == branch),
      remote_exists: remote,
      remote_name: remote.then(|| "origin".into()),
      remote_ref: remote.then(|| format!("origin/{branch}")),
    })
  }

  fn gitignore_suggestions(&self, _: &str) -> Vec<String> {
    self.suggestions.clone()
  }

  fn new_workspace(
    &self,
    _: &str,
    branch: &str,
    source: Option<&str>,
    metadata: Option<&str>,
  ) -> Result<i64, String> {
    self.record(format!(
      "create {branch} from {} {}",
      source.unwrap_or("-"),
      metadata.unwrap_or("-")
    ));
    if self.failing.contains(&"new_workspace") {
      return Err("create failed".into());
    }
    let mut created = self.created.lock().unwrap();
    let id = 100 + created.len() as i64;
    created.push(workspace_dialogs::workspace(id, branch, None));
    Ok(id)
  }

  fn move_changes(
    &self,
    _: &str,
    source: &str,
    destination: &str,
    request: crate::core::WorkspaceMoveRequest,
  ) -> Result<(), String> {
    self.record(format!(
      "move {source} -> {destination} files={:?} commits={:?}",
      request.files, request.commits
    ));
    if self.failing.contains(&"move_changes") {
      return Err("move failed".into());
    }
    Ok(())
  }

  fn retarget(
    &self,
    _: &str,
    workspace: &crate::local_db::Workspace,
    target: &str,
  ) -> Result<(), String> {
    self.record(format!("retarget {} -> {target}", workspace.branch_name));
    Ok(())
  }

  fn apply_stash(&self, _: &str, stash_id: i64, branch: &str) -> Result<(), String> {
    self.record(format!("apply stash {stash_id} onto {branch}"));
    if self.failing.contains(&"apply_stash") {
      return Err("stash failed".into());
    }
    Ok(())
  }

  fn rename_workspace(
    &self,
    _: &str,
    workspace_id: i64,
    branch: &str,
    dry_run: bool,
  ) -> Result<crate::core::RenameWorkspaceResult, String> {
    self.record(format!(
      "rename {workspace_id} -> {branch}{}",
      if dry_run { " (dry run)" } else { "" }
    ));
    let taken = self.local_branches.iter().any(|b| b == branch);
    // As the backend's `validate_branch_name`.
    let invalid = workspace_dialog_data::branch_name_error(branch, &[]);
    Ok(crate::core::RenameWorkspaceResult {
      success: !taken && invalid.is_none(),
      message: match invalid {
        Some(message) => message,
        None if taken => format!("Branch '{branch}' already exists"),
        None => String::new(),
      },
      workspace: None,
      updated_children_ids: Vec::new(),
    })
  }
}

impl workspace_ops::WorkspaceActionsHost for FakeHost {
  fn delete_workspace(&self, _: &str, workspace_id: i64) -> Result<(), String> {
    self.record(format!("delete workspace {workspace_id}"));
    if self.failing.contains(&"delete_workspace") {
      return Err("busy".into());
    }
    Ok(())
  }

  fn archive_workspace(&self, _: &str, workspace_id: i64) -> Result<(), String> {
    self.record(format!("archive workspace {workspace_id}"));
    if self.failing.contains(&"archive_workspace") {
      return Err(format!("cannot archive {workspace_id}"));
    }
    Ok(())
  }
}

fn agent_comment(suggested: Option<&str>) -> crate::local_db::AgentReviewComment {
  serde_json::from_value(serde_json::json!({
    "id": "ag", "repo_path": "/repo", "target_type": "workspace_diff", "target_id": "7",
    "file_path": "a.rs", "hunk_id": null, "start_line": 2, "end_line": 2, "side": "new",
    "comment_text": "Use one line", "suggested_replacement": suggested, "status": "open",
    "source": "local-agent", "created_at": "", "resolved_at": null
  }))
  .expect("agent comment")
}

mod account_screen;
mod allocs;
mod changes_large;
mod changes_screen;
mod code_files_large;
mod code_screen;
mod commit_diffs;
mod commits_large;
mod commits_screen;
mod conflicts;
mod dialogs;
mod diff_extremes;
mod file_browser;
mod fuzz;
mod github;
mod gutter_drag;
mod home_screen;
mod idle;
mod input_latency;
mod kept_tabs;
mod linear_view;
mod logs_explorer;
mod logs_screen;
mod merge_screen;
mod onboarding_screen;
mod open_workspaces;
mod pr_review;
mod prefetch;
mod review_comments;
mod settings_screen;
mod skills_screen;
mod stack_panel;
mod tab_badges;
mod terminal_perf;
mod terminal_screen;
mod testing_library;
mod text_editing;
mod virtual_chaos;
mod virtual_lists;
mod workspace_actions;
mod workspace_dialogs;
