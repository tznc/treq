use std::{
  cell::RefCell,
  collections::HashMap,
  rc::Rc,
  sync::{Arc, Mutex},
};

use gpui::{Entity, Modifiers, MouseButton, TestAppContext, VisualTestContext};

use super::*;

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
  /// What the folder picker returns, and why starting a dev container fails.
  picked: Option<String>,
  container_error: Option<String>,
  /// The Overview screen's machines and recent repositories.
  listing: machines::Listing,
  /// What a host machine says is installed on it.
  host_tools: Option<Result<Vec<crate::core::PrerequisiteStatus>, String>>,
  /// What `check_prerequisites` reports.
  installed: Vec<(&'static str, bool)>,
  /// App and repository settings, by key.
  settings: Mutex<settings_data::Values>,
  /// What `.treq/config.yaml` sets.
  config: crate::repo_config::RepoConfig,
  setup_script: String,
  /// The skill registry, as `skill_catalog` returns it.
  skills: Vec<crate::core::skills::SkillCatalogSkillView>,
  /// What the workspace header reads.
  loaded: workspace_data::Loaded,
  pr: workspace_data::PrCache,
  /// What the merge preview reads, and whether merging fails.
  merge: merge::Preview,
  merge_error: Option<String>,
  /// What the Code tab reads.
  overview: code_data::Overview,
  history: Option<crate::jj::JjLogResult>,
  workspaces: Vec<crate::local_db::Workspace>,
  files: Vec<String>,
  /// What the Commits tab reads.
  diff: Option<crate::jj::JjRevisionDiff>,
  /// Deferred file diffs `commit_file_diff` loads.
  file_diffs: Vec<crate::jj::JjFileDiff>,
  description: String,
  /// What the Changes tab reads, and the repository's `.gitattributes`.
  working: Option<crate::jj::JjRevisionDiff>,
  /// Replaces `working` once set: the files edited on disk meanwhile.
  edited: Mutex<Option<crate::jj::JjRevisionDiff>>,
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
  /// page (`Err` refuses it), and the merge queue (opt-in and entries).
  github_repos: Option<Result<Vec<crate::core::account::GithubRepository>, String>>,
  oauth: Option<Result<String, crate::core::supabase::ApiError>>,
  /// `None` fails the read, as when it never answers.
  queue: Mutex<Option<(bool, Vec<merge_queue_data::QueueEntry>)>>,
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

impl merge::MergeHost for FakeHost {
  fn merge_preview(&self, _: &str, workspace_id: i64) -> merge::Preview {
    self.record(format!("read merge {workspace_id}"));
    self.merge.clone()
  }

  fn merge(
    &self,
    _: &str,
    workspace_id: i64,
    message: &str,
    strategy: merge::Strategy,
  ) -> Result<(), String> {
    self.record(format!("merge {workspace_id} {strategy:?} {message:?}"));
    self.merge_error.clone().map_or(Ok(()), Err)
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

  fn warm_file_index(&self, _: &str, workspace_id: Option<i64>, _: &str) {
    self.record(format!("read file index {workspace_id:?}"));
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
  ) -> Result<crate::core::WorkspaceFileHunksBatch, String> {
    self.record(format!("read hunks {paths:?}"));
    let working = self.working().ok_or("no changes")?;
    Ok(crate::core::WorkspaceFileHunksBatch {
      snapshot_token: "t".into(),
      files: paths
        .iter()
        .map(|path| crate::core::WorkspaceFileHunksBatchFile {
          path: path.clone(),
          content_hash: format!("h-{path}"),
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

  fn sessions(&self, _: &str) -> Result<Vec<crate::local_db::Session>, String> {
    Ok(self.sessions.clone())
  }

  fn supporting_repos(&self, _: &str) -> Vec<String> {
    self.supporting.clone()
  }

  fn shell_command(&self) -> Option<String> {
    Some("export PATH=\"/bin:$PATH\"".into())
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

  fn cloud_action(&self, action: account_data::CloudAction) {
    match action {
      account_data::CloudAction::Refresh => self.record("read cloud workspace".into()),
      action => self.record(format!("cloud {action:?}")),
    }
  }
}

impl merge_queue_data::MergeQueueHost for FakeHost {
  fn merge_queue(
    &self,
    full_name: &str,
  ) -> Result<(bool, Vec<merge_queue_data::QueueEntry>), String> {
    self.record(format!("read queue {full_name}"));
    self
      .queue
      .lock()
      .unwrap()
      .clone()
      .ok_or_else(|| "unread".to_string())
  }

  fn dequeue(&self, full_name: &str, branches: &[String]) -> Result<(), String> {
    self.record(format!("dequeue {full_name} {}", branches.join(",")));
    Ok(())
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
    let (screen, cx) = cx.add_window_view(|_, cx| Screen::new(snapshot, sink, popups, host, cx));
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

#[test]
fn builds_the_checklist_from_the_check() {
  let list = checklist::from_statuses("Before you start", None, true);
  assert!(list.validating);
  assert_eq!(list.groups.len(), 3);
  // Nothing is reported missing before the first check returns.
  assert!(!list.groups[1].error);
  assert!(list
    .groups
    .iter()
    .flat_map(|g| &g.tools)
    .all(|t| t.installed.is_none()));

  let statuses = [("git", false), ("claude", false), ("gh", true)].map(|(binary, installed)| {
    crate::core::PrerequisiteStatus {
      binary: binary.into(),
      installed,
    }
  });
  let list = checklist::from_statuses("Before you start", Some(&statuses), false);
  let [required, agents, optional] = &list.groups[..] else {
    panic!("three groups");
  };
  assert_eq!(required.label, "Required");
  assert!(required.tools[0].required);
  assert_eq!(required.tools[0].installed, Some(false));
  assert!(agents.error, "no agent CLI is installed");
  assert_eq!(agents.tools.len(), 4);
  assert_eq!(optional.tools[0].installed, Some(true));
  assert_eq!(
    optional.tools[0].note.as_deref(),
    Some("Needed for pull requests and issues")
  );
  // The binaries are the ones the backend checks, in its order.
  let binaries: Vec<_> = list
    .groups
    .iter()
    .flat_map(|g| &g.tools)
    .map(|t| t.binary.as_str())
    .collect();
  assert_eq!(binaries, crate::core::PREREQUISITE_BINARIES);
}

#[test]
fn serializes_actions_as_react_reads_them() {
  let json = serde_json::to_value(ScreenAction::OpenUrl {
    url: "https://cli.github.com/".into(),
  })
  .expect("serialize");
  assert_eq!(
    json,
    serde_json::json!({"kind": "openUrl", "url": "https://cli.github.com/"})
  );
  assert_eq!(
    serde_json::to_value(ScreenAction::RecheckPrerequisites).expect("serialize"),
    serde_json::json!({"kind": "recheckPrerequisites"})
  );
}

#[gpui::test]
fn the_window_models_file_picker_reports_a_dismissal(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, workspace_route());
  let scope = file_picker::Scope {
    repo_path: "/repo".into(),
    workspace_id: None,
    workspace_path: "/repo".into(),
  };
  h.screen
    .update(&mut h.cx, |s, cx| s.set_file_picker(Some(scope), cx));
  assert!(h.drawn("file-picker"));
  h.click_backdrop();
  assert_eq!(
    h.sent().last(),
    Some(&ScreenAction::Press {
      id: "filePicker.close".into()
    })
  );
  h.screen
    .update(&mut h.cx, |s, cx| s.set_file_picker(None, cx));
  assert!(!h.drawn("file-picker"));
}

#[gpui::test]
fn the_ui_zoom_scales_pixel_lengths_as_css_zoom_does(cx: &mut TestAppContext) {
  use crate::gpui_shell::ui::{apply_zoom, UiZoom};
  let mut card = |zoom: u32| {
    let mut h = Harness::new(cx, onboarding());
    h.cx.update(|window, cx| {
      cx.set_global(UiZoom(zoom));
      apply_zoom(window, cx);
      window.refresh();
    });
    h.cx.run_until_parked();
    h.cx
      .debug_bounds("onboarding-open-repo")
      .expect("card")
      .size
  };
  let (normal, zoomed) = (card(100), card(150));
  assert!(
    (f32::from(zoomed.height) - f32::from(normal.height) * 1.5).abs() < 1.,
    "{normal:?} {zoomed:?}"
  );
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
      },
      Machine {
        id: "ssh:ep-1".into(),
        place: Place::Ssh {
          endpoint_id: "ep-1".into(),
        },
        name: "Dev box".into(),
        detail: "me@devbox".into(),
      },
      Machine {
        id: "cloud".into(),
        place: Place::Cloud {
          endpoint_id: None,
          status: machines::CLOUD_NOT_CREATED.into(),
        },
        name: "Cloud workspace".into(),
        detail: "Not created yet".into(),
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

#[gpui::test]
fn onboarding_runs_its_own_actions(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    installed: vec![("git", false), ("gh", true)],
    listing: listing(),
    ..FakeHost::default()
  });
  let mut h = Harness::with_host(cx, onboarding(), host.clone());
  // This computer's tools are checked as the screen opens, shown in its
  // popover.
  assert!(!h.drawn("install-git"));
  h.click("machine-tools-local");
  assert!(h.drawn("install-git"));
  assert!(!h.drawn("install-gh"));
  h.click("prerequisites-recheck");
  h.click("install-git");
  h.click("onboarding-open-repo");
  h.click("recent-local-0");
  assert_eq!(
    host.calls(),
    [
      "check",
      "check",
      "open https://git-scm.com/downloads",
      "open_local /home/me/app",
    ]
  );
  // The window model asks for the folder.
  assert_eq!(h.sent(), vec![ScreenAction::OpenRepo]);
}

#[gpui::test]
fn onboarding_groups_recent_repositories_by_machine(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    listing: listing(),
    ..FakeHost::default()
  });
  let mut h = Harness::with_host(cx, onboarding(), host.clone());
  for selector in [
    "onboarding-machines",
    "machine-local",
    "machine-ssh:ep-1",
    "machine-cloud",
    "machine-devcontainer:c1",
    "recent-local-0",
    "recent-ssh:ep-1-0",
  ] {
    assert!(h.drawn(selector), "{selector}");
  }
  assert!(!h.drawn("onboarding-no-recents"));
  // A remote repository opens through the window, by its saved record; a
  // machine's row connects to it.
  h.click("recent-ssh:ep-1-0");
  h.click("machine-ssh:ep-1");
  // A dev container opens its repository here.
  h.click("machine-devcontainer:c1");
  h.click("machine-local");
  // Before its workspace runs, the cloud row sets it up.
  h.click("machine-cloud");
  assert_eq!(
    h.sent(),
    vec![
      press("machines.openSaved:r1"),
      press("machines.connect:ep-1"),
      ScreenAction::OpenRepo,
      press("machines.cloud"),
    ]
  );
  assert_eq!(host.calls(), ["check", "open_local /home/me/web"]);
}

#[gpui::test]
fn onboarding_offers_sign_in_instead_of_a_cloud_row_when_signed_out(cx: &mut TestAppContext) {
  let mut signed_out = listing();
  signed_out.machines.retain(|m| m.id != "cloud");
  signed_out.signed_out = true;
  let host = Arc::new(FakeHost {
    listing: signed_out,
    ..FakeHost::default()
  });
  let mut h = Harness::with_host(cx, onboarding(), host.clone());
  assert!(!h.drawn("machine-cloud"));
  h.click("onboarding-sign-in");
  assert!(host
    .calls()
    .contains(&"open http://localhost:3001/sign-in?source=desktop".to_string()));
  let mut h = Harness::with_host(
    cx,
    onboarding(),
    Arc::new(FakeHost {
      listing: listing(),
      ..FakeHost::default()
    }),
  );
  assert!(
    !h.drawn("onboarding-sign-in"),
    "signed in: the cloud row instead"
  );
  assert!(h.drawn("machine-cloud"));
}

#[gpui::test]
fn onboarding_checks_a_host_only_when_its_popover_opens(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    listing: listing(),
    host_tools: Some(Ok(vec![
      crate::core::PrerequisiteStatus {
        binary: "git".into(),
        installed: true,
      },
      crate::core::PrerequisiteStatus {
        binary: "tmux".into(),
        installed: false,
      },
    ])),
    ..FakeHost::default()
  });
  let mut h = Harness::with_host(cx, onboarding(), host.clone());
  assert_eq!(host.calls(), ["check"]);
  h.click("machine-tools-ssh:ep-1");
  assert!(h.drawn("machine-tools-popover"));
  // jj and tmux are required on a host; Git is there.
  assert!(h.drawn("install-tmux"));
  assert!(h.drawn("install-jj"));
  assert!(!h.drawn("install-git"));
  assert!(host.calls().iter().any(|c| c.starts_with("tools Ssh")));
  // Removing the host closes the popover and lists the machines again.
  h.click("machine-remove");
  assert!(!h.drawn("machine-tools-popover"));
  // In-app confirmation first: Cancel keeps the host.
  h.answer("Cancel");
  assert!(!host.calls().iter().any(|c| c.starts_with("remove Ssh")));
  h.click("machine-tools-ssh:ep-1");
  h.click("machine-remove");
  h.answer("Remove host");
  assert!(host.calls().iter().any(|c| c.starts_with("remove Ssh")));
}

#[gpui::test]
fn onboarding_says_why_a_host_could_not_be_checked(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    listing: listing(),
    ..FakeHost::default()
  });
  let mut h = Harness::with_host(cx, onboarding(), host);
  h.click("machine-tools-ssh:ep-1");
  assert!(h.drawn("machine-tools-error"));
}

#[gpui::test]
fn onboarding_without_recents_says_where_they_appear(cx: &mut TestAppContext) {
  let mut h = Harness::with_host(cx, onboarding(), Arc::new(FakeHost::default()));
  assert!(h.drawn("onboarding-no-recents"));
}

fn onboarding_with_form(form: serde_json::Value) -> ScreenSnapshot {
  serde_json::from_value(serde_json::json!({
    "kind": "onboarding",
    "dark": false,
    "remoteRepo": form,
  }))
  .expect("onboarding")
}

#[gpui::test]
fn onboarding_draws_the_remote_repository_form(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    listing: listing(),
    ..FakeHost::default()
  });
  let mut h = Harness::with_host(
    cx,
    onboarding_with_form(serde_json::json!({
      "endpointId": "ep-1",
      "selector": true,
      "saved": [{ "id": "r1", "display_name": "app", "canonical_remote_path": "/srv/app" }],
      "path": "/srv/new",
      "probe": { "is_repo": false, "needs_clone": true },
      "cloneUrl": "",
      "confirmInit": false,
      "error": "probe failed",
    })),
    host,
  );
  for selector in [
    "onboarding-remote-form",
    "remote-repository-selector",
    "remote-browser",
    "remote-repo-not-a-repo",
    "remote-repo-clone-url",
    "remote-repo-init",
    "remote-repo-error",
  ] {
    assert!(h.drawn(selector), "{selector}");
  }
  // The group lists the saved repositories; the form only opens new ones.
  assert!(!h.drawn("remote-saved-repo"));
  assert!(!h.drawn("remote-repo-open"));
  assert!(!h.drawn("remote-repo-cutoff"));
  // Clone and init already show for the folder.
  assert!(!h.drawn("remote-repo-use-folder"));
  h.click("remote-repo-confirm-init");
  // Initialize waits for the confirmation; Clone for a URL.
  h.click("remote-repo-init");
  h.click("remote-repo-clone");
  assert_eq!(
    h.sent(),
    vec![ScreenAction::Field {
      field: "remoteRepo.confirmInit".into(),
      value: true.into()
    }]
  );
}

fn browsed_form(path: &str, is_repo: bool, show_hidden: bool) -> serde_json::Value {
  serde_json::json!({
    "endpointId": "ep-1",
    "selector": true,
    "path": path,
    "showHidden": show_hidden,
    "listing": {
      "path": path,
      "parent": "/home",
      "is_repo": is_repo,
      "entries": [
        { "name": ".config", "path": format!("{path}/.config"), "hidden": true, "is_repo": false },
        { "name": "app", "path": format!("{path}/app"), "hidden": false, "is_repo": true },
      ],
    },
  })
}

#[gpui::test]
fn the_remote_browser_lists_folders_and_hides_dot_folders(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    listing: listing(),
    ..FakeHost::default()
  });
  let mut h = Harness::with_host(
    cx,
    onboarding_with_form(browsed_form("/home/dev", false, false)),
    host,
  );
  assert!(h.drawn("remote-browse-folder-app"));
  assert!(
    !h.drawn("remote-browse-folder-.config"),
    "hidden by default"
  );
  assert!(h.drawn("remote-repo-use-folder"));
  assert!(!h.drawn("remote-repo-open"));
  h.click("remote-browse-folder-app");
  h.click("remote-browse-crumb-1");
  h.click("remote-browse-home");
  h.click("remote-browse-hidden");
  h.click("remote-repo-use-folder");
  assert_eq!(
    h.sent(),
    vec![
      field("remoteRepo.browse", "/home/dev/app".into()),
      field("remoteRepo.browse", "/home".into()),
      field("remoteRepo.browse", "~".into()),
      field("remoteRepo.showHidden", true.into()),
      press("remoteRepo.useFolder"),
    ]
  );
}

#[gpui::test]
fn the_remote_browser_shows_hidden_folders_and_opens_a_repository(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    listing: listing(),
    ..FakeHost::default()
  });
  let mut h = Harness::with_host(
    cx,
    onboarding_with_form(browsed_form("/home/dev/app", true, true)),
    host,
  );
  assert!(h.drawn("remote-browse-folder-.config"));
  assert!(!h.drawn("remote-repo-use-folder"));
  h.click("remote-repo-open");
  assert_eq!(h.sent(), vec![press("remoteRepo.open")]);
}

#[gpui::test]
fn onboarding_offers_reauthentication_when_cut_off(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    listing: listing(),
    ..FakeHost::default()
  });
  let mut h = Harness::with_host(
    cx,
    onboarding_with_form(serde_json::json!({
      "endpointId": "ep-1",
      "cutoff": "session_ended",
      "selector": true,
    })),
    host,
  );
  assert!(h.drawn("remote-repo-cutoff"));
  assert!(!h.drawn("remote-repository-selector"));
  h.click("remote-repo-reauthenticate");
  assert_eq!(h.sent(), vec![press("remoteRepo.reauthenticate")]);
}

#[gpui::test]
fn onboarding_adds_machines_from_their_sections(cx: &mut TestAppContext) {
  let mut empty = listing();
  empty
    .machines
    .retain(|m| matches!(m.place, machines::Place::Local));
  empty.ssh = true;
  empty.containers = true;
  let host = Arc::new(FakeHost {
    listing: empty,
    picked: Some("/home/me/web".into()),
    container_error: Some("The Dev Containers CLI is not installed.".into()),
    ..FakeHost::default()
  });
  let mut h = Harness::with_host(cx, onboarding(), host.clone());
  // Both sections show while empty, each with its +.
  assert!(h.drawn("machines-add-ssh"));
  assert!(h.drawn("machines-add-container"));
  h.click("machines-add-ssh");
  assert_eq!(h.sent(), vec![press("machines.addSsh")]);
  h.click("machines-add-container");
  h.cx.run_until_parked();
  assert!(host
    .calls()
    .contains(&"container up /home/me/web".to_string()));
  assert!(h.drawn("machines-container-status"), "why it did not start");

  let mut off = listing();
  off
    .machines
    .retain(|m| matches!(m.place, machines::Place::Local));
  let host = Arc::new(FakeHost {
    listing: off,
    ..FakeHost::default()
  });
  let mut h = Harness::with_host(cx, onboarding(), host);
  assert!(!h.drawn("machines-add-ssh"), "the previews are off");
  assert!(!h.drawn("machines-add-container"));
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

fn field(field: &str, value: serde_json::Value) -> ScreenAction {
  ScreenAction::Field {
    field: field.into(),
    value,
  }
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

#[gpui::test]
fn settings_saves_the_repository_itself(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::with_settings(&[(
    "/repo:auto_review_trigger",
    "on-push",
  )]));
  let mut h = Harness::with_host(cx, settings(), host.clone());
  let screen = drawn_settings(&mut h);
  let repo = screen.repository.expect("repository tab");
  assert!(!repo.loading);
  assert_eq!(repo.current_branch.as_deref(), Some("main"));
  assert_eq!(repo.fields.branch_name_pattern, "treq/{name}");
  // The trigger's old name reads as the new one.
  assert_eq!(repo.fields.auto_review_trigger, "on-pull");
  assert_eq!(repo.model_options[0].label, "Use Application Default");

  h.click("settings-tab-application");
  h.click("settings-auto-push");
  h.click("settings-save");
  h.click("settings-close");
  // React still owns the tab and closing the page.
  assert_eq!(
    h.sent(),
    vec![
      field("settings.tab", "application".into()),
      press("settings.close"),
    ]
  );
  h.cx.run_until_parked();
  assert_eq!(
    host.writes(),
    [
      "set repo /repo branch_name_pattern=treq/{name} included_copy_files= default_model= default_agent= auto_push=true review_prompt= review_agent= auto_review_trigger=on-pull",
      "notify repoSettingsChanged",
      "notify toast Settings saved",
    ]
  );
}

#[gpui::test]
fn settings_takes_typed_values(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, settings());
  // Controls report edits from inside the screen's update.
  h.click("settings-branch-pattern");
  h.cx.simulate_input("x");
  let fields = drawn_settings(&mut h).repository.expect("drawn").fields;
  assert!(fields.branch_name_pattern.contains('x'), "{fields:?}");
}

#[gpui::test]
fn settings_disables_what_the_yaml_file_sets(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    config: crate::repo_config::RepoConfig {
      branch_name_pattern: Some("team/{name}".into()),
      review_agent: Some("codex".into()),
      ..Default::default()
    },
    ..FakeHost::default()
  });
  let mut h = Harness::with_host(cx, settings(), host.clone());
  let repo = drawn_settings(&mut h).repository.expect("repository tab");
  assert_eq!(repo.managed, ["branchNamePattern", "reviewAgent"]);
  assert_eq!(
    repo.yaml_card,
    Some(settings::YamlCard {
      error: None,
      loading: false
    })
  );
  h.click("settings-yaml-reload");
  h.cx.run_until_parked();
  let reads = host
    .calls()
    .iter()
    .filter(|c| *c == "read config /repo")
    .count();
  assert_eq!(reads, 2);
}

#[gpui::test]
fn settings_saves_the_application_itself(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::with_settings(&[
    ("terminal_font_size", "13"),
    ("ui_zoom", "120"),
  ]));
  let mut h = Harness::with_host(cx, settings_route("application"), host.clone());
  let app = drawn_settings(&mut h).application.expect("application tab");
  // A size older builds allowed shows as the nearest preset.
  assert_eq!(app.font_size, "12");
  assert_eq!(app.zoom, 120.);
  assert!(app.notify_agent_finished);
  assert_eq!(app.conflict_marker_style, "git");
  assert_eq!(app.checklist.title, "Command-line tools");

  let view = h
    .screen
    .read_with(&h.cx, |s, _| s.settings.clone().expect("drawn"));
  let send = |h: &mut Harness, action: ScreenAction| {
    view.update(&mut h.cx, |view, cx| view.handle(action, cx));
  };
  send(&mut h, field("app.theme", "dark".into()));
  send(&mut h, field("app.fontSize", "16".into()));
  send(&mut h, field("app.defaultAgent", "codex".into()));
  h.click("settings-save");
  h.cx.run_until_parked();
  assert_eq!(
    host.writes(),
    [
      "set theme=dark",
      "notify settingsChanged",
      "set default_model= default_agent=codex conflict_marker_style=git notify_agent_finished=true terminal_font_size=16 ui_zoom=120",
      "notify settingsChanged",
      "notify toast Settings saved",
    ]
  );
}

#[gpui::test]
fn settings_runs_only_the_saved_setup_script(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    setup_script: "brew install jj".into(),
    ..FakeHost::default()
  });
  let mut h = Harness::with_host(cx, settings_route("application"), host.clone());
  let setup = drawn_settings(&mut h)
    .application
    .expect("drawn")
    .setup_script;
  assert!(setup.can_run);
  assert_eq!(setup.status, "Never run");
  // The Run row is below the fold of the test window.
  let view = h
    .screen
    .read_with(&h.cx, |s, _| s.settings.clone().expect("drawn"));
  view.update(&mut h.cx, |view, cx| {
    view.handle(press("app.setupScript.run"), cx)
  });
  h.cx.run_until_parked();
  assert_eq!(host.writes(), ["run setup"]);

  view.update(&mut h.cx, |view, cx| {
    view.handle(field("app.setupScript", "brew install gh".into()), cx)
  });
  let setup = drawn_settings(&mut h)
    .application
    .expect("drawn")
    .setup_script;
  assert!(!setup.can_run);
  assert_eq!(setup.status, "Save settings to run the edited script.");
}

#[gpui::test]
fn settings_switches_previews_itself(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::with_settings(&[(
    "feature_preview.skillsInstallation",
    "false",
  )]));
  let mut h = Harness::with_host(cx, settings_route("preview"), host.clone());
  let screen = drawn_settings(&mut h);
  assert!(!screen.tabs.contains(&settings::Tab::Skills));
  h.click("settings-preview-skillsInstallation");
  h.click("settings-preview-docs-logs");
  h.cx.run_until_parked();
  assert!(drawn_settings(&mut h).tabs.contains(&settings::Tab::Skills));
  assert_eq!(
    host.writes(),
    [
      "set feature_preview.skillsInstallation=true",
      "notify settingsChanged",
      "open http://localhost:3001/docs/concepts/logs",
    ]
  );
}

#[gpui::test]
fn settings_scrolls_its_content(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, settings());
  h.cx
    .simulate_resize(gpui::size(gpui::px(1100.), gpui::px(400.)));
  h.cx.run_until_parked();
  let before = h.cx.debug_bounds("settings-auto-push").expect("drawn");
  let scroll = h.cx.debug_bounds("settings-scroll").expect("drawn");
  h.cx.simulate_event(gpui::ScrollWheelEvent {
    position: scroll.center(),
    delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-200.))),
    ..Default::default()
  });
  h.cx.run_until_parked();
  let after = h.cx.debug_bounds("settings-auto-push").expect("drawn");
  assert!(after.origin.y < before.origin.y, "{before:?} -> {after:?}");
}

#[gpui::test]
fn settings_fields_are_as_tall_as_react_ones(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, settings());
  h.cx.run_until_parked();
  // `ui/input.tsx` and the `<select>`s are `h-10`.
  for selector in ["settings-branch-pattern", "settings-repo-default-model"] {
    let bounds = h.cx.debug_bounds(selector).expect("drawn");
    assert_eq!(bounds.size.height, gpui::px(40.), "{selector}");
  }
}

const WORKSPACE_JSON: &str = r#"{
  "header": {
    "branch": "feature/login",
    "home": false,
    "linear": {"key": "ENG-12", "url": "https://linear.app/t/ENG-12", "title": "Login"},
    "target": {"branch": "main", "loading": false, "disabled": false},
    "homeTarget": null,
    "stack": null,
    "push": null,
    "viewPr": {"number": 42, "state": "OPEN", "draft": false},
    "createPr": null,
    "statusLoading": false,
    "sync": {"ahead": 2, "behind": 0, "disabled": false, "spinning": false},
    "checks": {"status": "passed", "count": "7/7", "name": "Checks: Treq 3/3, GitHub 4/4"},
    "mergeQueue": null,
    "merge": {"passing": true, "disabled": false, "tooltip": null},
    "details": true,
    "more": {"disabled": false}
  },
  "tabRow": {
    "tab": "overview",
    "commits": {"kind": "count", "count": 3, "tone": "muted"},
    "changes": {"count": 5, "tone": "uncommitted"},
    "logs": false,
    "busy": null,
    "loc": {"insertions": 12, "deletions": 4}
  }
}"#;

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
    "mergeQueue": null,
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

fn press(id: &str) -> ScreenAction {
  ScreenAction::Press { id: id.into() }
}

#[test]
fn workspace_header_is_what_react_drew() {
  let expected: workspace::Workspace = serde_json::from_str(WORKSPACE_JSON).expect("parse");
  let (header, tab_row) = workspace_data::workspace_view(
    route_of(&workspace_route()),
    &feature_loaded(),
    &open_pr(),
    workspace_data::Pending::None,
  );
  assert_eq!(header, expected.header);
  assert_eq!(tab_row, expected.tab_row);
}

#[test]
fn workspace_header_offers_the_home_rebase() {
  let route: ScreenSnapshot = serde_json::from_value(route_json(None, "feat/home")).expect("route");
  let loaded = workspace_data::Loaded {
    home_ahead: 2,
    ..Default::default()
  };
  let (header, _) = workspace_data::workspace_view(
    route_of(&route),
    &loaded,
    &workspace_data::PrCache::default(),
    workspace_data::Pending::None,
  );
  assert!(header.home);
  assert_eq!(header.branch, "feat/home");
  let target = header.home_target.expect("home target");
  assert_eq!(
    target.rebase.expect("rebase").tooltip,
    "Rebase feat/home onto main (2 new commits)"
  );
  assert_eq!(
    header.stack.expect("stack").tooltip,
    "Create stacked workspace from feat/home"
  );
}

#[gpui::test]
fn workspace_header_sends_what_it_does_not_do_itself(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, feature_host(), workspace_route());
  for selector in [
    "workspace-view-pr",
    "workspace-view-pr-menu",
    "workspace-merge",
    "workspace-more",
  ] {
    h.click(selector);
  }
  h.click("workspace-linear");
  h.cx.run_until_parked();
  // View PR and More run in GPUI (`workspace/actions.rs`).
  assert_eq!(h.sent(), vec![press("workspace.merge")]);
  assert_eq!(
    host.writes(),
    vec![
      "notify open",
      "open https://github.com/tznc/treq/pull/42",
      "open https://linear.app/t/ENG-12",
    ]
  );
}

#[gpui::test]
fn workspace_header_github_buttons_copy_their_link(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, feature_host(), workspace_route());
  let pr = "https://github.com/tznc/treq/pull/42";
  for (selector, url) in [
    ("workspace-view-pr", pr.to_string()),
    ("workspace-view-pr-menu", pr.to_string()),
    ("workspace-checks", format!("{pr}/checks")),
  ] {
    h.right_click(selector);
    h.pick("Copy link");
    let copied = h.cx.read_from_clipboard().and_then(|c| c.text());
    assert_eq!(copied.as_deref(), Some(url.as_str()), "{selector}");
  }
  assert!(h.sent().is_empty());
  assert_eq!(host.writes(), vec!["notify toast Link copied"; 3]);
}

#[gpui::test]
fn workspace_header_syncs_itself(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, feature_host(), workspace_route());
  h.click("workspace-sync");
  h.cx.run_until_parked();
  assert!(h.sent().is_empty());
  assert_eq!(
    host.writes(),
    vec![
      "sync Some(7)",
      "notify toast Sync failed",
      "notify workspaceChanged",
    ]
  );
  // The window reads again on `workspaceChanged` and bumps the route's
  // revision, which reads the header again
  // (`workspace_header_reads_again_when_the_model_refreshes`): the header
  // doesn't read it twice.
  let reads = host
    .calls()
    .iter()
    .filter(|c| c.starts_with("read workspace "))
    .count();
  assert_eq!(reads, 1);
}

#[gpui::test]
fn workspace_header_reads_again_when_the_model_refreshes(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, feature_host(), workspace_route());
  assert!(h.drawn("workspace-sync"));
  h.update(|s| s.dark = true);
  h.update(|s| match &mut s.view {
    View::Workspace(route) => route.revision = 3,
    _ => unreachable!(),
  });
  h.cx.run_until_parked();
  let reads: Vec<_> = host
    .calls()
    .into_iter()
    .filter(|c| c.starts_with("read workspace "))
    .collect();
  assert_eq!(reads, vec!["read workspace 0", "read workspace 3"]);
  assert!(host.calls().contains(&"read pr feature/login".to_string()));
}

#[gpui::test]
fn workspace_tabs_switch_through_react(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, feature_host(), workspace_route());
  assert!(h.drawn("commits-tab-count"));
  assert!(h.drawn("review-change-count"));
  assert!(h.drawn("loc-diff-marker"));
  assert!(!h.drawn("workspace-tab-logs"));
  h.click("workspace-tab-changes");
  h.cx.run_until_parked();
  assert_eq!(h.sent(), vec![field("workspace.tab", "changes".into())]);
}

#[gpui::test]
fn home_workspace_opens_the_branch_switcher(cx: &mut TestAppContext) {
  let route = serde_json::from_value(route_json(None, "main")).expect("route");
  let (mut h, _) = workspace_harness(cx, FakeHost::default(), route);
  h.click("workspace-branch");
  h.click("workspace-stack");
  h.cx.run_until_parked();
  assert_eq!(
    h.sent(),
    vec![press("workspace.branchSwitcher"), press("workspace.stack")]
  );
}

fn skill(
  id: &str,
  name: &str,
  installed: Option<&str>,
) -> crate::core::skills::SkillCatalogSkillView {
  serde_json::from_value(serde_json::json!({
    "id": id,
    "name": name,
    "description": format!("{name} helps"),
    "source": "anthropics/skills",
    "category": "docs",
    "url": format!("https://github.com/anthropics/skills/{id}"),
    "installed": installed.map(|scope| serde_json::json!({
      "id": id,
      "name": name,
      "checksum": "abc",
      "scope": scope,
    })),
  }))
  .expect("skill")
}

fn skills_harness(cx: &mut TestAppContext) -> (Harness, Arc<FakeHost>) {
  let host = Arc::new(FakeHost {
    skills: vec![
      skill("pdf", "PDF", Some("application")),
      skill("docx", "Word", None),
    ],
    ..FakeHost::default()
  });
  let h = Harness::with_host(cx, settings_route("skills"), host.clone());
  h.cx
    .simulate_resize(gpui::size(gpui::px(1400.), gpui::px(900.)));
  (h, host)
}

fn settings_view(h: &mut Harness) -> Entity<settings::SettingsView> {
  h.cx.run_until_parked();
  h.screen
    .read_with(&h.cx, |screen, _| screen.settings.clone())
    .expect("settings drawn")
}

fn settings_act(h: &mut Harness, action: ScreenAction) {
  let view = settings_view(h);
  view.update(&mut h.cx, |view, cx| view.handle(action, cx));
  h.cx.run_until_parked();
}

#[gpui::test]
fn skills_tab_lists_and_filters_the_registry(cx: &mut TestAppContext) {
  let (mut h, host) = skills_harness(cx);
  let skills = drawn_settings(&mut h).skills.expect("skills tab");
  assert_eq!(skills.installed, 1);
  let names: Vec<_> = skills.rows.iter().map(|r| r.name.as_str()).collect();
  assert_eq!(names, ["PDF", "Word"]);
  assert_eq!(skills.rows[0].installed.as_deref(), Some("application"));
  assert!(h.drawn("skill-uninstall-pdf"));
  assert!(h.drawn("skill-install-docx"));
  assert_eq!(
    host.calls().iter().filter(|c| *c == "read skills").count(),
    1
  );

  settings_act(&mut h, field("skills.query", "WORD".into()));
  let names: Vec<_> = drawn_settings(&mut h)
    .skills
    .expect("skills tab")
    .rows
    .into_iter()
    .map(|r| r.name)
    .collect();
  assert_eq!(names, ["Word"]);

  settings_act(&mut h, field("skills.query", "".into()));
  settings_act(&mut h, field("skills.scope", "application".into()));
  let rows = drawn_settings(&mut h).skills.expect("skills tab").rows;
  assert_eq!(rows.len(), 1);
  assert_eq!(rows[0].id, "pdf");
}

#[gpui::test]
fn skills_install_asks_where_then_toasts_a_failure(cx: &mut TestAppContext) {
  let (mut h, host) = skills_harness(cx);
  h.click("skill-install-docx");
  assert!(h.drawn("skill-install-dialog"));
  assert_eq!(
    drawn_settings(&mut h)
      .skills
      .expect("skills tab")
      .dialog
      .map(|d| d.name),
    Some("Word".into())
  );
  h.click("skill-dialog-repository");
  h.cx.run_until_parked();
  assert!(!h.drawn("skill-install-dialog"));
  assert_eq!(
    host.writes(),
    [
      "install docx Repository Some(\"/repo\")",
      "notify toast Skill library",
    ]
  );
  let reads = host.calls().iter().filter(|c| *c == "read skills").count();
  assert_eq!(reads, 2, "it reads the registry again");
  assert!(h.sent().is_empty());
}

#[gpui::test]
fn skills_uninstall_and_move_between_levels(cx: &mut TestAppContext) {
  let (mut h, host) = skills_harness(cx);
  h.click("skill-uninstall-pdf");
  h.cx.run_until_parked();
  settings_act(&mut h, field("skills.level:pdf", "repository".into()));
  h.click("skill-source-pdf");
  h.cx.run_until_parked();
  assert_eq!(
    host.writes(),
    [
      "uninstall pdf",
      "scope pdf Repository",
      "open https://github.com/anthropics/skills/pdf",
    ]
  );
}

#[gpui::test]
fn skills_dialog_cancels(cx: &mut TestAppContext) {
  let (mut h, host) = skills_harness(cx);
  h.click("skill-install-docx");
  h.click("skill-dialog-cancel");
  assert!(!h.drawn("skill-install-dialog"));
  assert!(host.writes().is_empty());
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

#[gpui::test]
fn account_signs_in_through_the_browser(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::default());
  let route = account_route("account");
  let mut h = Harness::with_host(cx, route, host.clone());
  h.set_account(account_state(
    serde_json::json!({ "signInError": "token expired" }),
  ));
  let account = drawn_settings(&mut h).account.expect("account tab");
  let settings::AccountTab::SignedOut { error, .. } = account else {
    panic!("signed out: {account:?}");
  };
  assert_eq!(error.as_deref(), Some("Sign-in failed: token expired"));
  h.click("account-sign-in");
  h.cx.run_until_parked();
  assert_eq!(
    host.writes(),
    ["open http://localhost:3001/sign-in?source=desktop"]
  );
  assert!(h.sent().is_empty());
}

#[gpui::test]
fn account_shows_loading_and_signing_in(cx: &mut TestAppContext) {
  let route = account_route("account");
  let mut h = Harness::new(cx, route);
  h.set_account(account_state(serde_json::json!({ "loading": true })));
  assert_eq!(
    drawn_settings(&mut h).account,
    Some(settings::AccountTab::Loading)
  );
  h.set_account(account_state(serde_json::json!({ "signingIn": true })));
  assert_eq!(
    drawn_settings(&mut h).account,
    Some(settings::AccountTab::SigningIn)
  );
  assert!(h.drawn("account-signing-in"));
}

#[gpui::test]
fn account_trades_a_pasted_callback_token(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::default());
  let route = account_route("account");
  let mut h = Harness::with_host(cx, route, host.clone());
  h.set_account(account_state(serde_json::json!({})));
  settings_act(
    &mut h,
    field("account.callbackUrl", "treq://auth/callback?token=".into()),
  );
  h.click("account-dev-callback-go");
  let account = drawn_settings(&mut h).account.expect("account tab");
  let settings::AccountTab::SignedOut { dev, .. } = account else {
    panic!("signed out");
  };
  assert_eq!(
    dev.expect("dev builds offer it").error.as_deref(),
    Some("No token found in URL")
  );
  assert!(h.sent().is_empty());

  settings_act(
    &mut h,
    field(
      "account.callbackUrl",
      " treq://auth/callback?token=abc&x=1 ".into(),
    ),
  );
  h.click("account-dev-callback-go");
  h.cx.run_until_parked();
  settings_act(&mut h, field("account.callbackUrl", "expired".into()));
  h.click("account-dev-callback-go");
  h.cx.run_until_parked();
  assert_eq!(host.writes(), ["exchange abc", "exchange expired"]);
  assert!(h.sent().is_empty());
  let Some(settings::AccountTab::SignedOut { dev, .. }) = drawn_settings(&mut h).account else {
    panic!("signed out");
  };
  let dev = dev.expect("dev builds offer it");
  assert!(!dev.loading);
  assert_eq!(dev.error.as_deref(), Some("Token exchange failed"));
}

#[gpui::test]
fn account_shows_the_plan_and_signs_out(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::default());
  let mut user = ada();
  user["avatarUrl"] = "https://example.com/ada.png".into();
  let route = account_route("account");
  let mut h = Harness::with_host(cx, route, host.clone());
  h.set_account(account_state(
    serde_json::json!({ "user": user, "subscription": pro() }),
  ));
  let account = drawn_settings(&mut h).account.expect("account tab");
  let settings::AccountTab::SignedIn(signed_in) = account else {
    panic!("signed in: {account:?}");
  };
  assert_eq!(signed_in.name, "Ada");
  assert_eq!(signed_in.email.as_deref(), Some("ada@example.com"));
  assert!(signed_in.pro);
  assert_eq!(
    signed_in.status,
    Some(("Active".into(), settings::StatusTone::Good))
  );
  assert_eq!(
    signed_in.period,
    Some(("Renews on".into(), "11/1/2026".into()))
  );
  assert_eq!(signed_in.manage_label, "Manage Subscription");

  h.click("account-manage-subscription");
  h.click("account-sign-out");
  h.cx.run_until_parked();
  assert_eq!(
    host.writes(),
    [
      "fetch https://example.com/ada.png",
      "open http://localhost:3001/dashboard",
      "sign out"
    ]
  );
  assert!(host.calls().contains(&"read cloud workspace".to_string()));
  assert!(h.sent().is_empty());
}

#[gpui::test]
fn account_offers_pro_on_the_free_plan(cx: &mut TestAppContext) {
  let route = account_route("account");
  let mut h = Harness::new(cx, route);
  h.set_account(account_state(serde_json::json!({
    "user": ada(),
    "subscription": { "plan": "free", "status": "canceled", "currentPeriodEnd": null },
  })));
  let Some(settings::AccountTab::SignedIn(signed_in)) = drawn_settings(&mut h).account else {
    panic!("signed in");
  };
  assert!(!signed_in.pro);
  assert_eq!(
    signed_in.status,
    Some(("Canceling".into(), settings::StatusTone::Warn))
  );
  assert_eq!(signed_in.period, None);
  assert_eq!(signed_in.manage_label, "Upgrade to Pro");
}

fn cloud(patch: serde_json::Value) -> serde_json::Value {
  let mut cloud = serde_json::json!({
    "instance": null,
    "endpoint": null,
    "provisioningStage": null,
    "provisioningError": null,
    "usage": null,
    "usageError": null,
    "submitting": false,
  });
  if let (Some(cloud), Some(patch)) = (cloud.as_object_mut(), patch.as_object()) {
    for (key, value) in patch {
      cloud.insert(key.clone(), value.clone());
    }
  }
  cloud
}

fn signed_in_with_cloud(
  cx: &mut TestAppContext,
  host: Arc<FakeHost>,
  cloud: serde_json::Value,
) -> Harness {
  let mut h = Harness::with_host(cx, account_route("account"), host);
  h.set_account(account_state(
    serde_json::json!({ "user": ada(), "subscription": pro(), "cloud": cloud }),
  ));
  h
}

fn cloud_card(h: &mut Harness) -> settings::CloudCard {
  match drawn_settings(h).account {
    Some(settings::AccountTab::SignedIn(signed_in)) => signed_in.cloud.expect("cloud card"),
    other => panic!("signed in: {other:?}"),
  }
}

#[gpui::test]
fn cloud_workspace_creates_through_rust(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::default());
  let mut h = signed_in_with_cloud(cx, host.clone(), cloud(serde_json::json!({})));
  let card = cloud_card(&mut h);
  assert_eq!(card.instance, None);
  assert_eq!(
    card.create.as_ref().map(|c| c.label.as_str()),
    Some("Create cloud workspace")
  );
  h.click("cloud-create");
  assert_eq!(host.writes(), ["cloud Provision"]);
  assert!(h.sent().is_empty());

  h.set_account(account_state(serde_json::json!({
    "user": ada(),
    "cloud": cloud(serde_json::json!({ "provisioningStage": "Requesting provisioning..." })),
  })));
  let create = cloud_card(&mut h).create.expect("create");
  assert_eq!(create.label, "Creating...");
  assert!(create.disabled && create.spinning);
  assert_eq!(create.stage.as_deref(), Some("Requesting provisioning..."));
}

#[gpui::test]
fn cloud_workspace_shows_usage_and_deletes_after_confirming(cx: &mut TestAppContext) {
  let ready = cloud(serde_json::json!({
    "instance": { "status": "ready", "providerResourceId": "m-1", "diskQuotaGb": 5 },
    "endpoint": { "username": "treq", "hostname": "box.treq.dev", "port": 2222 },
    "usage": { "repositoryCount": 3, "workspaceCount": 7, "diskUsedBytes": 1932735283u64 },
  }));
  let host = Arc::new(FakeHost::default());
  let mut h = signed_in_with_cloud(cx, host.clone(), ready);
  h.cx
    .simulate_resize(gpui::size(gpui::px(1400.), gpui::px(1200.)));
  let instance = cloud_card(&mut h).instance.expect("instance");
  assert_eq!(instance.label, "Ready");
  assert_eq!(instance.endpoint.as_deref(), Some("treq@box.treq.dev:2222"));
  assert!(!instance.repairable);
  assert_eq!(
    instance.usage,
    Some(settings::CloudUsage::Report {
      summary: "7 workspaces across 3 repositories".into(),
      size: "1.8 GB of 5 GB".into(),
      percent: 36,
    })
  );
  assert!(!h.drawn("cloud-repair"));
  h.click("cloud-open-repositories");
  h.click("cloud-delete");
  assert!(h.drawn("cloud-delete-dialog"));
  h.click("cloud-delete-cancel");
  assert!(!h.drawn("cloud-delete-dialog"));
  h.click("cloud-delete");
  h.click("cloud-delete-confirm");
  assert!(!h.drawn("cloud-delete-dialog"));
  assert_eq!(host.writes(), ["cloud OpenRepositories", "cloud Delete"]);
  assert_eq!(h.sent(), [press("settings.close")]);
}

#[gpui::test]
fn cloud_workspace_repairs_and_upsells_pro(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::default());
  let degraded = cloud(serde_json::json!({
    "instance": { "status": "degraded", "providerResourceId": "m-1", "diskQuotaGb": 5 },
    "provisioningError": "[pro_required] Cloud workspaces need Pro.\nhttp 402",
  }));
  let mut h = signed_in_with_cloud(cx, host.clone(), degraded);
  let card = cloud_card(&mut h);
  assert!(card.instance.expect("instance").repairable);
  assert_eq!(
    card.pro_required.as_deref(),
    Some("Cloud workspaces need Pro.")
  );
  assert_eq!(card.error, None);
  h.click("cloud-repair");
  h.click("cloud-upgrade");
  h.cx.run_until_parked();
  assert!(h.sent().is_empty());
  assert_eq!(
    host.writes(),
    ["cloud Repair", "open http://localhost:3001/dashboard"]
  );

  h.set_account(account_state(serde_json::json!({
    "user": ada(),
    "cloud": cloud(serde_json::json!({
      "instance": { "status": "failed", "providerResourceId": null, "diskQuotaGb": 5 },
      "provisioningError": "[http_502] exec failed\nstack",
    })),
  })));
  let card = cloud_card(&mut h);
  assert_eq!(card.error.as_deref(), Some("[http_502] exec failed"));
  assert_eq!(
    card.create.map(|c| c.label),
    Some("Retry cloud workspace creation".into())
  );
}

#[gpui::test]
fn integrations_sign_in_when_signed_out(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::with_settings(&[(
    "feature_preview.linearIntegration",
    "false",
  )]));
  let route = account_route("integrations");
  let mut h = Harness::with_host(cx, route, host.clone());
  h.set_account(account_state(serde_json::json!({})));
  let integrations = drawn_settings(&mut h).integrations.expect("integrations");
  assert_eq!(integrations.github, settings::GithubSection::SignedOut);
  assert_eq!(integrations.linear, None);
  h.click("integrations-sign-in");
  h.cx.run_until_parked();
  assert_eq!(
    host.writes(),
    ["open http://localhost:3001/sign-in?source=desktop"]
  );
}

#[gpui::test]
fn integrations_list_public_repositories_on_the_free_plan(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::with_settings(&[(
    "feature_preview.linearIntegration",
    "false",
  )]));
  let repo = |id, name: &str, private| crate::core::account::GithubRepository {
    id,
    full_name: name.into(),
    private,
  };
  let host = Arc::new(FakeHost {
    github_repos: Some(Ok(vec![
      repo(1, "acme/public", false),
      repo(2, "acme/private", true),
    ])),
    ..Arc::into_inner(host).expect("one owner")
  });
  let route = account_route("integrations");
  let mut h = Harness::with_host(cx, route, host.clone());
  h.set_account(account_state(serde_json::json!({ "user": ada() })));
  let integrations = drawn_settings(&mut h).integrations.expect("integrations");
  assert_eq!(
    integrations.github,
    settings::GithubSection::SignedIn {
      scope: "Public repositories".into(),
      description: "1 enabled.".into(),
      repositories: vec![("acme/public".into(), false)],
    }
  );
  h.click("github-manage");
  h.cx.run_until_parked();
  assert_eq!(
    host.writes(),
    ["open http://localhost:3001/dashboard?tab=integrations"]
  );
}

fn linear_harness(
  cx: &mut TestAppContext,
  pro: bool,
  key: Option<&str>,
) -> (Harness, Arc<FakeHost>) {
  let mut settings = vec![("feature_preview.linearIntegration", "true")];
  settings.extend(key.map(|key| ("/repo:linear_api_key", key)));
  let host = Arc::new(FakeHost::with_settings(&settings));
  let subscription = if pro {
    self::pro()
  } else {
    serde_json::Value::Null
  };
  let route = account_route("integrations");
  let mut h = Harness::with_host(cx, route, host.clone());
  h.set_account(account_state(
    serde_json::json!({ "user": ada(), "subscription": subscription }),
  ));
  h.cx
    .simulate_resize(gpui::size(gpui::px(1400.), gpui::px(1200.)));
  (h, host)
}

fn linear_section(h: &mut Harness) -> settings::LinearSection {
  drawn_settings(h)
    .integrations
    .expect("integrations")
    .linear
    .expect("linear")
}

#[gpui::test]
fn linear_saves_an_api_key_to_the_repository(cx: &mut TestAppContext) {
  let (mut h, host) = linear_harness(cx, false, None);
  let linear = linear_section(&mut h);
  assert_eq!(linear.description, "Free plan: provide your API key");
  assert_eq!(linear.edit_label, "Add API Key");
  assert!(!linear.oauth);
  h.click("linear-api-key-edit");
  assert!(linear_section(&mut h).editing);
  settings_act(&mut h, field("linear.apiKey", "lin_api_1".into()));
  h.click("linear-api-key-save");
  h.cx.run_until_parked();
  assert_eq!(
    host.writes(),
    [
      "set repo /repo linear_api_key=lin_api_1",
      "notify repoSettingsChanged",
      "notify toast API Key saved",
    ]
  );
  let linear = linear_section(&mut h);
  assert!(!linear.editing);
  assert_eq!(linear.description, "Direct API access configured");
  assert_eq!(linear.edit_label, "Update API Key");
}

#[gpui::test]
fn linear_connects_via_oauth_on_pro(cx: &mut TestAppContext) {
  let (mut h, host) = linear_harness(cx, true, Some("lin_api_1"));
  let linear = linear_section(&mut h);
  assert!(linear.oauth);
  assert!(linear.key_priority_note);
  assert_eq!(
    linear.oauth_description,
    "API key takes priority, but you can also connect via OAuth"
  );
  h.click("linear-oauth");
  h.cx.run_until_parked();
  assert!(h.sent().is_empty());
  assert_eq!(
    host.writes(),
    ["linear oauth", "open https://linear.app/oauth/authorize"]
  );
  assert!(!linear_section(&mut h).oauth_loading);
}

#[gpui::test]
fn linear_oauth_refused_for_want_of_pro_offers_the_upgrade(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    oauth: Some(Err(crate::core::supabase::ApiError {
      status: 402,
      code: "pro_required".into(),
      message: "Pro".into(),
      correlation_id: None,
    })),
    ..FakeHost::with_settings(&[("feature_preview.linearIntegration", "true")])
  });
  let mut h = Harness::with_host(cx, account_route("integrations"), host.clone());
  h.cx
    .simulate_resize(gpui::size(gpui::px(1400.), gpui::px(1200.)));
  h.set_account(account_state(
    serde_json::json!({ "user": ada(), "subscription": pro() }),
  ));
  h.click("linear-oauth");
  h.cx.run_until_parked();
  assert_eq!(
    host.writes(),
    ["linear oauth", "notify toast Linear OAuth needs Pro"]
  );
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
    error: None,
  }
}

fn merge_harness(cx: &mut TestAppContext, host: FakeHost) -> (Harness, Arc<FakeHost>) {
  let host = Arc::new(host);
  let mut h = Harness::with_host(cx, merge_route(), host.clone());
  assert!(h.drawn("merge-confirm"));
  (h, host)
}

fn merge_view<T>(h: &mut Harness, f: impl FnOnce(&merge::MergeView) -> T) -> T {
  h.screen.read_with(&h.cx, |screen, cx| {
    f(screen.merge.as_ref().expect("merge view").read(cx))
  })
}

#[test]
fn merge_messages_follow_the_strategy() {
  let ws = merge_workspace(Some(r#"{"description": "Adds a thing"}"#));
  assert_eq!(
    merge::default_message(&ws, merge::Strategy::Merge),
    "Merge feature into develop\n\nAdds a thing"
  );
  assert_eq!(
    merge::default_message(&ws, merge::Strategy::Squash),
    "Adds a thing"
  );
  assert_eq!(merge::default_message(&ws, merge::Strategy::Rebase), "");
  let bare = merge_workspace(Some("not json"));
  assert_eq!(
    merge::default_message(&bare, merge::Strategy::Merge),
    "Merge feature into develop"
  );
}

#[gpui::test]
fn merge_preview_merges_then_hands_back(cx: &mut TestAppContext) {
  let (mut h, host) = merge_harness(
    cx,
    FakeHost {
      merge: merge_preview(2),
      ..FakeHost::default()
    },
  );
  assert!(h.drawn("merge-file-src/a.rs"));
  h.click("merge-confirm");
  h.cx.run_until_parked();
  assert_eq!(
    host.writes(),
    [
      "merge 7 Merge \"Merge feature into develop\\n\\nAdds a thing\"",
      "notify toast Workspace merged",
    ]
  );
  assert_eq!(
    h.sent(),
    vec![ScreenAction::Press {
      id: "mergePreview.merged".into()
    }]
  );
}

#[gpui::test]
fn merge_preview_picks_a_strategy(cx: &mut TestAppContext) {
  let (mut h, host) = merge_harness(
    cx,
    FakeHost {
      merge: merge_preview(2),
      merge_error: Some("conflicts".into()),
      ..FakeHost::default()
    },
  );
  h.click("merge-strategy");
  assert!(h.drawn("merge-strategy-menu"));
  h.click("merge-strategy-rebase");
  assert!(!h.drawn("merge-strategy-menu"));
  assert_eq!(
    merge_view(&mut h, |v| (v.strategy, v.message())),
    (merge::Strategy::Rebase, String::new())
  );
  h.click("merge-confirm");
  h.cx.run_until_parked();
  assert_eq!(
    host.writes(),
    ["merge 7 Rebase \"\"", "notify toast Merge failed"]
  );
  assert!(h.sent().is_empty());
  assert!(!merge_view(&mut h, |v| v.merging));
}

#[gpui::test]
fn merge_preview_keeps_what_was_typed(cx: &mut TestAppContext) {
  let (mut h, _) = merge_harness(
    cx,
    FakeHost {
      merge: merge_preview(1),
      ..FakeHost::default()
    },
  );
  h.screen.update(&mut h.cx, |screen, cx| {
    let view = screen.merge.clone().expect("merge view");
    view.update(cx, |view, _| view.draft = Some("Ship it".into()));
  });
  h.click("merge-strategy");
  h.click("merge-strategy-squash");
  assert_eq!(merge_view(&mut h, |v| v.message()), "Ship it");
}

#[gpui::test]
fn merge_preview_collapses_files_and_goes_back(cx: &mut TestAppContext) {
  let (mut h, _) = merge_harness(
    cx,
    FakeHost {
      merge: merge_preview(1),
      ..FakeHost::default()
    },
  );
  assert!(h.drawn("merge-hunk-0-0"));
  h.click("merge-file-src/a.rs");
  assert!(!h.drawn("merge-hunk-0-0"));
  h.click("merge-back");
  assert_eq!(
    h.sent(),
    vec![ScreenAction::Press {
      id: "mergePreview.cancel".into()
    }]
  );
}

#[gpui::test]
fn merge_preview_without_commits_cannot_merge(cx: &mut TestAppContext) {
  let (mut h, host) = merge_harness(
    cx,
    FakeHost {
      merge: merge::Preview {
        error: Some("no such revision".into()),
        ..merge::Preview::default()
      },
      ..FakeHost::default()
    },
  );
  h.click("merge-confirm");
  h.click("merge-strategy");
  assert!(!h.drawn("merge-strategy-menu"));
  assert_eq!(host.writes(), ["notify toast Failed to load merge preview"]);
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

#[gpui::test]
fn stack_line_counts_stay_on_one_line(cx: &mut TestAppContext) {
  let host = FakeHost {
    workspaces: [
      workspace_row("feature/login"),
      serde_json::json!({"id": 8, "branch_name": "feature/logout", "target_branch": "feature/login"}),
    ]
    .into_iter()
    .map(|row| {
      let mut base = workspace_row("feature/login");
      base.as_object_mut().expect("row").extend(row.as_object().expect("row").clone());
      serde_json::from_value(base).expect("workspace")
    })
    .collect(),
    ..code_host()
  };
  let (mut h, _) = workspace_harness(cx, host, code_route());
  h.cx.run_until_parked();
  let counts = h
    .cx
    .debug_bounds("workspace-loc-indicator")
    .expect("indicator");
  // One `text-xs` line, as React's grid of `ch` columns keeps it.
  assert!(counts.size.height <= gpui::px(16.), "{counts:?}");
}

#[gpui::test]
fn code_tab_draws_files_history_and_readme(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, code_host(), code_route());
  for selector in [
    "workspace-code",
    "agent-task-input",
    "workspace-description",
    "code-go-to-file",
    "code-entry-src",
    "code-entry-README.md",
    "code-readme",
    "commit-item-a",
    "commit-item-b",
  ] {
    assert!(h.drawn(selector), "{selector}");
  }
  let calls = host.calls();
  assert!(
    calls.contains(&"read overview Some(7)".to_string()),
    "{calls:?}"
  );
  assert!(
    calls.iter().any(|c| c.starts_with("read commits Some(7)")),
    "{calls:?}"
  );
}

#[gpui::test]
fn code_tab_says_a_remote_file_tree_is_unavailable(cx: &mut TestAppContext) {
  let host = FakeHost {
    failing: vec!["remote_overview"],
    ..code_host()
  };
  let (mut h, _) = workspace_harness(cx, host, code_route());
  assert!(h.drawn("code-files-unavailable"));
  assert!(!h.drawn("code-files-empty"));
  assert!(h.drawn("code-go-to-file"), "Go to file still opens files");
}

#[gpui::test]
fn code_tab_says_when_commits_fail_to_load(cx: &mut TestAppContext) {
  let host = FakeHost {
    history: None,
    ..code_host()
  };
  let (mut h, _) = workspace_harness(cx, host, code_route());
  h.cx.run_until_parked();
  assert!(
    h.drawn("code-history-error"),
    "not stuck on Loading commits..."
  );
  assert!(!h.drawn("code-history-error-popover"));
  h.click("code-history-show-error");
  assert!(
    h.drawn("code-history-error-popover"),
    "Show error opens the error"
  );
  assert!(!h.drawn("commit-item-a"));
}

#[gpui::test]
fn code_tab_hands_navigation_to_react(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, code_host(), code_route());
  h.click("code-entry-src");
  h.click("commit-item-b");
  h.click("code-go-to-file");
  h.cx.run_until_parked();
  assert_eq!(
    fields(&h, "code.open"),
    vec![serde_json::json!({"name": "src", "isDirectory": true})]
  );
  assert_eq!(fields(&h, "code.commit"), vec![serde_json::json!("b")]);
  assert!(h.sent().contains(&press("code.filePicker")));
}

#[gpui::test]
fn code_tab_starts_a_task_in_the_workspace(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, code_host(), code_route());
  h.click("task-input");
  h.cx.simulate_input("fix the login button");
  h.click("task-submit");
  h.cx.run_until_parked();
  assert!(
    host
      .writes()
      .contains(&r#"create session Some(7) "fix the login button""#.to_string()),
    "{:?}",
    host.writes()
  );
  let created = host.started_sessions();
  assert_eq!(created.len(), 1, "{:?}", h.sent());
  assert_eq!(created[0]["sessionId"], 31);
  assert_eq!(created[0]["workspaceId"], 7);
  assert_eq!(created[0]["pendingPrompt"], "fix the login button");
  assert_eq!(created[0]["permissionMode"], "acceptEdits");
  assert_eq!(created[0]["agent"], "claude");
}

#[gpui::test]
fn code_tab_runs_home_tasks_in_a_new_workspace(cx: &mut TestAppContext) {
  let mut host = code_host();
  host.loaded.workspace = None;
  let mut route = route_json(None, "main");
  route["code"] = true.into();
  let route = serde_json::from_value(route).expect("home route");
  let (mut h, host) = workspace_harness(cx, host, route);
  assert!(h.drawn("task-new-workspace"));
  h.click("task-input");
  h.cx.simulate_input("fix login");
  h.click("task-submit");
  h.cx.run_until_parked();
  let writes = host.writes();
  let create = writes
    .iter()
    .find(|c| c.starts_with("create workspace "))
    .unwrap_or_else(|| panic!("{writes:?}"));
  // `treq/fix-login` is taken, so the branch gets a suffix.
  assert!(
    create.starts_with("create workspace treq/fix-login-"),
    "{create}"
  );
  assert!(
    writes.contains(&r#"create session Some(9) "fix login""#.to_string()),
    "{writes:?}"
  );
  assert_eq!(host.started_sessions()[0]["workspaceId"], 9);
}

fn home_code_route() -> ScreenSnapshot {
  let mut route = route_json(None, "main");
  route["code"] = true.into();
  serde_json::from_value(route).expect("home route")
}

#[gpui::test]
fn code_tab_turning_the_new_workspace_toggle_off_runs_home_tasks_at_home(cx: &mut TestAppContext) {
  let mut host = code_host();
  host.loaded.workspace = None;
  let (mut h, host) = workspace_harness(cx, host, home_code_route());
  h.click("task-new-workspace");
  h.cx.run_until_parked();
  assert!(
    host
      .writes()
      .contains(&"set repo /repo home_task_new_workspace=false".to_string()),
    "{:?}",
    host.writes()
  );
  h.click("task-input");
  h.cx.simulate_input("fix login");
  h.click("task-submit");
  h.cx.run_until_parked();
  let writes = host.writes();
  assert!(
    !writes.iter().any(|c| c.starts_with("create workspace ")),
    "{writes:?}"
  );
  assert!(
    writes.contains(&r#"create session None "fix login""#.to_string()),
    "{writes:?}"
  );
}

#[gpui::test]
fn code_tab_remembers_running_home_tasks_at_home(cx: &mut TestAppContext) {
  let mut host = code_host();
  host.loaded.workspace = None;
  host
    .settings
    .lock()
    .unwrap()
    .insert("/repo:home_task_new_workspace".into(), Some("false".into()));
  let (mut h, host) = workspace_harness(cx, host, home_code_route());
  assert!(h.drawn("task-new-workspace"));
  h.click("task-input");
  h.cx.simulate_input("fix login");
  h.click("task-submit");
  h.cx.run_until_parked();
  let writes = host.writes();
  assert!(
    !writes.iter().any(|c| c.starts_with("create workspace ")),
    "{writes:?}"
  );
  assert!(
    writes.contains(&r#"create session None "fix login""#.to_string()),
    "{writes:?}"
  );
}

#[gpui::test]
fn code_tab_lists_submodules_with_their_pin_and_syncs_them_on_request(cx: &mut TestAppContext) {
  let mut host = code_host();
  host.loaded.workspace = None;
  host.overview.entries.push(crate::core::WorkspaceEntry {
    name: "vendor/lib".into(),
    path: "/repo/vendor/lib".into(),
    is_directory: true,
    modified_at: None,
    submodule_pin: Some("abcdef0123456789".into()),
    submodule_synced: Some(false),
    status: None,
  });
  let (mut h, host) = workspace_harness(cx, host, home_code_route());
  assert!(h.drawn("submodule-row-vendor/lib"));
  h.click("submodule-sync-box-vendor/lib");
  h.cx.run_until_parked();
  assert!(
    host
      .writes()
      .contains(&"sync submodule vendor/lib true".to_string()),
    "{:?}",
    host.writes()
  );
}

/// Draws the window once without running the reads a route change started,
/// so the frame shows what a user sees before they land.
fn draw_now(h: &mut Harness) {
  h.cx.update(|window, cx| window.draw(cx).clear(cx));
}

#[gpui::test]
fn code_tab_draws_what_it_read_when_returning_to_it(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, code_host(), code_route());
  assert!(h.drawn("workspace-description"));
  h.update(|s| s.view = commits_route().view);
  assert!(h.drawn("workspace-commits"));
  h.update(|s| s.view = code_route().view);
  draw_now(&mut h);
  assert!(h.cx.debug_bounds("workspace-description").is_some());
  assert!(h.cx.debug_bounds("workspace-overview-skeleton").is_none());
}

#[gpui::test]
fn switching_workspaces_keeps_the_last_one_drawn_until_the_next_loads(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, code_host(), code_route());
  assert!(h.drawn("workspace-description"));
  h.update(|s| {
    let mut route = route_json(Some(8), "main");
    route["code"] = true.into();
    route["frameHeight"] = 120.into();
    s.view = serde_json::from_value::<ScreenSnapshot>(route)
      .expect("route")
      .view;
  });
  draw_now(&mut h);
  for selector in [
    "workspace-branch",
    "workspace-tab-row",
    "workspace-description",
  ] {
    assert!(h.cx.debug_bounds(selector).is_some(), "{selector}");
  }
  assert!(h.cx.debug_bounds("workspace-overview-skeleton").is_none());
}

fn task_text(h: &mut Harness) -> Option<String> {
  h.cx.run_until_parked();
  h.screen.read_with(&h.cx, |s, cx| {
    let code = s.workspace.as_ref()?.read(cx).code.clone()?;
    Some(code.read(cx).task_text.clone())
  })
}

#[gpui::test]
fn file_picker_searches_the_workspace_and_inserts_the_picked_file(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, code_host(), code_route());
  h.click("task-add");
  h.click("task-add-file");
  assert!(h.drawn("file-picker"));
  h.cx.simulate_input("log");
  h.cx
    .executor()
    .advance_clock(std::time::Duration::from_millis(500));
  assert!(host.calls().contains(&r#"read files "log""#.to_string()));
  assert!(h.drawn("file-picker-item-0") && h.drawn("file-picker-item-1"));
  h.click("file-picker-item-1");
  assert!(!h.drawn("file-picker"));
  assert_eq!(task_text(&mut h).as_deref(), Some("@src/logout.rs "));
}

#[gpui::test]
fn file_picker_lists_nothing_for_an_unknown_name_and_closes_on_escape(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, code_host(), code_route());
  h.click("task-add");
  h.click("task-add-file");
  h.cx.simulate_input("nothing-here");
  h.cx
    .executor()
    .advance_clock(std::time::Duration::from_millis(500));
  assert!(h.drawn("file-picker"));
  assert!(!h.drawn("file-picker-item-0"));
  h.cx.simulate_keystrokes("escape");
  assert!(!h.drawn("file-picker"));
  assert_eq!(task_text(&mut h).as_deref(), Some(""));
}

#[gpui::test]
fn code_tab_offers_no_new_workspace_toggle_inside_a_workspace(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, code_host(), code_route());
  assert!(h.drawn("agent-task-input"));
  assert!(!h.drawn("task-new-workspace"));
}

#[gpui::test]
fn clicking_empty_space_blurs_the_focused_input(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, code_host(), code_route());
  let focused = |h: &mut Harness| {
    h.cx.run_until_parked();
    h.cx.update(|window, cx| window.focused(cx).is_some())
  };
  // The task input starts focused.
  assert!(focused(&mut h));
  // A mouse-down on a spot nothing focuses takes the focus off it, as in a
  // browser, so a key such as "?" then reaches the app's shortcuts.
  h.cx.simulate_click(
    gpui::point(gpui::px(1395.), gpui::px(895.)),
    Modifiers::none(),
  );
  assert!(!focused(&mut h));
  // Clicking an input focuses it again.
  h.click("task-input");
  assert!(focused(&mut h));
}

#[gpui::test]
fn code_tab_suggests_files_for_mentions(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, code_host(), code_route());
  h.click("task-input");
  h.cx.simulate_input("look at @log");
  h.cx
    .executor()
    .advance_clock(std::time::Duration::from_millis(500));
  assert!(h.drawn("task-mentions"));
  assert!(h.drawn("task-mention-0") && h.drawn("task-mention-1"));
  assert!(host.calls().contains(&r#"read files "log""#.to_string()));
  h.click("task-mention-1");
  h.cx.run_until_parked();
  let text = h.screen.read_with(&h.cx, |s, cx| {
    let code = s.workspace.as_ref()?.read(cx).code.clone()?;
    Some(code.read(cx).task_text.clone())
  });
  assert_eq!(text.as_deref(), Some("look at @src/logout.rs "));
}

#[gpui::test]
fn code_tab_saves_the_description(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, code_host(), code_route());
  h.click("workspace-description-edit");
  assert!(h.drawn("workspace-description-input"));
  h.cx.simulate_input("!");
  h.click("workspace-description-save");
  h.cx.run_until_parked();
  assert!(
    host
      .writes()
      .iter()
      .any(|c| c.starts_with("set description 7 ") && c.contains('!')),
    "{:?}",
    host.writes()
  );
  assert!(!h.drawn("workspace-description-input"));
}

#[gpui::test]
fn code_tab_shows_the_title_above_the_description_not_in_the_header(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, code_host(), code_route());
  h.cx.run_until_parked();
  let title = h.cx.debug_bounds("workspace-title").expect("title");
  let description = h
    .cx
    .debug_bounds("workspace-description")
    .expect("description");
  assert!(
    title.bottom() <= description.top(),
    "{title:?} {description:?}"
  );
  let header = h.cx.debug_bounds("workspace-header").expect("header");
  assert!(!header.intersects(&title), "{header:?} {title:?}");
}

#[gpui::test]
fn code_tab_saves_the_title(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, code_host(), code_route());
  h.click("workspace-title-edit");
  assert!(h.drawn("workspace-title-input"));
  h.cx.simulate_input("!");
  h.click("workspace-title-save");
  h.cx.run_until_parked();
  assert!(
    host
      .writes()
      .iter()
      .any(|c| c.starts_with("set title 7 ") && c.contains('!')),
    "{:?}",
    host.writes()
  );
  assert!(!h.drawn("workspace-title-input"));
}

#[gpui::test]
fn code_tab_discards_title_edits_on_cancel_and_escape(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, code_host(), code_route());
  h.click("workspace-title-edit");
  h.cx.simulate_input("!");
  h.click("workspace-title-cancel");
  assert!(!h.drawn("workspace-title-input"));
  h.click("workspace-title-edit");
  h.cx.simulate_input("!");
  h.cx.simulate_keystrokes("escape");
  h.cx.run_until_parked();
  assert!(!h.drawn("workspace-title-input"));
  assert!(
    !host.writes().iter().any(|c| c.starts_with("set title")),
    "{:?}",
    host.writes()
  );
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

#[gpui::test]
fn commits_tab_lists_commits_and_opens_their_diffs(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, commits_host(), commits_route());
  for selector in [
    "workspace-commits",
    "commit-row-a",
    "commit-row-b",
    "shift-commits-to-now",
  ] {
    assert!(h.drawn(selector), "{selector}");
  }
  assert!(!h.drawn("workspace-code"));
  assert!(host
    .calls()
    .contains(&"read log Some(7) None None".to_string()));
  h.click("commit-row-a");
  assert!(h.drawn("commit-file-row"));
  assert!(host.calls().contains(&"read diff a".to_string()));
  // The newest commit can be undone; every own commit edited and moved.
  for selector in [
    "edit-description-a",
    "undo-commit-a",
    "revert-commit-a",
    "edit-timestamp-a",
    "move-commit-a",
    "stash-commit-button",
    "delete-commit-a",
  ] {
    assert!(h.drawn(selector), "{selector}");
  }
  h.click("commit-file-row");
  assert!(!h.drawn("add-comment-0-1"));
  h.click("commit-row-a");
  assert!(!h.drawn("commit-file-row"));
}

#[gpui::test]
fn commits_tab_rewrites_commits_in_the_home_repo(cx: &mut TestAppContext) {
  let mut route = route_json(None, "main");
  route["code"] = true.into();
  route["tab"] = "commits".into();
  let route = serde_json::from_value(route).expect("home commits route");
  let (mut h, host) = workspace_harness(cx, commits_host(), route);
  h.click("commit-row-a");
  for selector in ["edit-description-a", "revert-commit-a", "delete-commit-a"] {
    assert!(h.drawn(selector), "{selector}");
  }
  h.click("revert-commit-a");
  h.answer("Revert commit");
  assert!(host.writes().contains(&"revert a".to_string()));
  h.click("edit-description-a");
  assert!(h.drawn("edit-description-dialog"));
  assert!(host.calls().contains(&"read description a".to_string()));
}

#[gpui::test]
fn commits_tab_abandons_once_confirmed(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, commits_host(), commits_route());
  h.click("commit-row-a");
  h.click("delete-commit-a");
  h.answer("Cancel");
  assert!(!host.writes().iter().any(|w| w.starts_with("abandon")));
  assert!(!h.drawn("confirm"));
}

#[gpui::test]
fn commits_tab_removes_an_abandoned_commit(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, commits_host(), commits_route());
  h.click("commit-row-a");
  h.click("delete-commit-a");
  assert!(!host.writes().iter().any(|w| w.starts_with("abandon")));
  h.answer("Delete commit");
  assert!(host.writes().contains(&"abandon a".to_string()));
  h.cx
    .executor()
    .advance_clock(std::time::Duration::from_millis(300));
  assert!(!h.drawn("commit-row-a"));
  assert!(h.drawn("commit-row-b"));
}

#[gpui::test]
fn commits_tab_opens_the_commit_react_asked_for(cx: &mut TestAppContext) {
  let mut route = commits_route();
  if let View::Workspace(r) = &mut route.view {
    r.scroll_to_commit = Some("b".into());
  }
  let (mut h, host) = workspace_harness(cx, commits_host(), route);
  assert!(h.drawn("commit-file-row"));
  assert!(host.calls().contains(&"read diff b".to_string()));
  assert_eq!(fields(&h, "commits.scrolled").len(), 1);
}

#[gpui::test]
fn commits_tab_sends_a_line_comment_to_an_agent(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, commits_host(), commits_route());
  h.click("commit-row-a");
  h.click("add-comment-0-1");
  assert!(h.drawn("comment-input"));
  h.cx.simulate_input("why?");
  h.click("comment-edit");
  h.cx.run_until_parked();
  assert!(
    host
      .writes()
      .contains(&r#"create session Some(7) "Code Comment""#.to_string()),
    "{:?}",
    host.writes()
  );
  let created = host.started_sessions();
  assert_eq!(
    created[0]["pendingPrompt"],
    "src/a.rs:2 (commit a)\n```\n+b\n```\n> why?\n"
  );
  assert_eq!(created[0]["permissionMode"], "acceptEdits");
  assert!(!h.drawn("comment-input"));
}

#[gpui::test]
fn commits_tab_edits_a_description(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, commits_host(), commits_route());
  h.click("commit-row-a");
  h.click("edit-description-a");
  assert!(h.drawn("edit-description-dialog"));
  assert!(host.calls().contains(&"read description a".to_string()));
  h.cx.simulate_input("!");
  h.click("description-save");
  h.cx.run_until_parked();
  assert!(
    host
      .writes()
      .iter()
      .any(|w| w.starts_with("describe a ") && w.contains('!')),
    "{:?}",
    host.writes()
  );
  assert!(!h.drawn("edit-description-dialog"));
}

#[gpui::test]
fn commits_tab_shifts_the_stack_from_the_timestamp_dialog(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, commits_host(), commits_route());
  h.click("commit-row-a");
  h.click("edit-timestamp-a");
  assert!(h.drawn("edit-timestamp-dialog"));
  // Nothing to apply until a duration is entered.
  h.click("timestamp-apply");
  h.cx.run_until_parked();
  assert!(!host.writes().iter().any(|w| w.starts_with("shift a")));
  h.click("shift-stack-to-now");
  h.cx.run_until_parked();
  assert!(host.writes().contains(&"shift to now 7".to_string()));
  assert!(!h.drawn("edit-timestamp-dialog"));
}

#[gpui::test]
fn commits_tab_resolves_conflicts_with_an_agent(cx: &mut TestAppContext) {
  let mut host = commits_host();
  if let Some(log) = host.history.as_mut() {
    log.commits[0].has_conflicts = true;
  }
  let (mut h, host) = workspace_harness(cx, host, commits_route());
  assert!(h.drawn("resolve-conflicts-banner"));
  h.click("resolve-conflicts-button");
  assert!(h.drawn("resolve-conflicts-dialog"));
  assert!(host
    .calls()
    .contains(&r#"start resolve Some(7) Some(["a"])"#.to_string()));
  type_into(&mut h, "resolve-conflicts-prompt", "keep both sides");
  h.click("resolve-conflicts-submit");
  h.cx.run_until_parked();
  assert!(
    host
      .writes()
      .contains(&r#"create session Some(7) "Resolve a""#.to_string()),
    "{:?}",
    host.writes()
  );
  let created = host.started_sessions();
  assert_eq!(created[0]["workspacePath"], "/r/cwd");
  assert_eq!(created[0]["pendingPrompt"], "resolve: keep both sides");
  assert!(!h.drawn("resolve-conflicts-dialog"));
}

#[gpui::test]
fn commits_tab_resolve_dialog_shows_why_it_cannot_resolve(cx: &mut TestAppContext) {
  let mut host = commits_host();
  if let Some(log) = host.history.as_mut() {
    log.commits[0].has_conflicts = true;
  }
  host.failing = vec!["start_resolve"];
  let (mut h, host) = workspace_harness(cx, host, commits_route());
  h.click("resolve-conflicts-button");
  h.cx.run_until_parked();
  assert!(h.drawn("resolve-conflicts-error"));
  type_into(&mut h, "resolve-conflicts-prompt", "keep both sides");
  h.click("resolve-conflicts-submit");
  h.cx.run_until_parked();
  assert!(host.started_sessions().is_empty());
  assert!(h.drawn("resolve-conflicts-dialog"));
}

#[gpui::test]
fn commits_tab_discards_the_tentative_working_copy(cx: &mut TestAppContext) {
  let mut host = commits_host();
  if let Some(log) = host.history.as_mut() {
    log.tentative_working_copy = serde_json::from_value(serde_json::json!({
      "commit": commit("wc", 1, 0),
      "workspace_label": "login"
    }))
    .ok();
  }
  let (mut h, host) = workspace_harness(cx, host, commits_route());
  h.click("commit-row-wc");
  h.click("delete-tentative-changes");
  h.cx.run_until_parked();
  let writes = host.writes();
  assert!(
    writes.iter().any(|w| w.starts_with("snapshot ")),
    "{writes:?}"
  );
  assert!(
    writes.iter().any(|w| w.starts_with("discard ")),
    "{writes:?}"
  );
  assert!(!h.drawn("commit-row-wc"));
}

#[gpui::test]
fn commits_tab_hands_moves_to_react(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, commits_host(), commits_route());
  h.click("commit-row-a");
  h.click("move-commit-a");
  h.click("move-commit-existing");
  h.cx.run_until_parked();
  assert_eq!(fields(&h, "commits.moveToExisting")[0]["change_id"], "a");
  assert!(!h.drawn("move-commit-menu"));
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

fn remote_chrome(state: &str, cut_off: bool) -> serde_json::Value {
  serde_json::json!({
    "displayName": "app",
    "terminal": true,
    "state": state,
    "cutOff": cut_off,
    "notices": ["Merging a remote workspace is not available yet."],
  })
}

#[gpui::test]
fn a_remote_repository_hides_stash_all(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host(), changes_route());
  assert!(h.drawn("stash-all-changes"));
  h.update(|s| {
    if let View::Workspace(route) = &mut s.view {
      route.remote_chrome = serde_json::from_value(remote_chrome("online", false)).ok();
    }
  });
  h.cx.run_until_parked();
  assert!(!h.drawn("stash-all-changes"));
  assert!(h.drawn("discard-all-changes"));
}

#[gpui::test]
fn a_remote_repository_draws_dashboards_strip(cx: &mut TestAppContext) {
  let mut route = route_json(Some(7), "main");
  route["remoteChrome"] = remote_chrome("offline", false);
  let snapshot = serde_json::from_value(route).expect("remote route");
  let (mut h, _) = workspace_harness(cx, code_host(), snapshot);
  for selector in [
    "remote-strip",
    "remote-terminal-link",
    "remote-refresh-link",
    "remote-status-banner",
    "remote-banner-reconnect",
    "remote-banner-refresh",
    "remote-capability-notice",
    "workspace-header",
  ] {
    assert!(h.drawn(selector), "{selector}");
  }
  assert!(!h.drawn("remote-banner-wake"));
  assert!(!h.drawn("remote-cutoff-overlay"));
  h.click("remote-terminal-link");
  h.click("remote-banner-reconnect");
  h.click("remote-refresh-link");
  assert_eq!(
    h.sent(),
    vec![
      press("remote.terminal"),
      press("remote.reconnect"),
      press("remote.refresh")
    ]
  );
}

#[gpui::test]
fn a_cut_off_remote_repository_dims_the_window(cx: &mut TestAppContext) {
  let mut route = route_json(Some(7), "main");
  route["remoteChrome"] = remote_chrome("cutoff", true);
  let snapshot = serde_json::from_value(route).expect("remote route");
  let (mut h, _) = workspace_harness(cx, code_host(), snapshot);
  assert!(h.drawn("remote-cutoff-overlay"));
  assert!(h.drawn("remote-status-banner"));
  assert!(
    !h.drawn("remote-banner-refresh"),
    "no Refresh while cut off"
  );
  // The strip's links take no clicks.
  h.click("remote-terminal-link");
  h.click("remote-refresh-link");
  assert!(h.sent().is_empty(), "{:?}", h.sent());
}

#[gpui::test]
fn changes_tab_lists_files_and_their_diffs(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  for selector in [
    "changes-diff-viewer",
    "file-row-a.rs",
    "file-row-b.rs",
    "file-card-a.rs",
    "file-card-b.rs",
    "commit-button",
  ] {
    assert!(h.drawn(selector), "{selector}");
  }
  assert!(!h.drawn("workspace-code"));
  assert!(host.calls().contains(&"read workspace diff 7".to_string()));
  assert!(
    host.calls().iter().any(|c| c.starts_with("read hunks")),
    "{:?}",
    host.calls()
  );
}

fn changes_host_with_generated() -> FakeHost {
  let mut host = changes_host();
  host.gitattributes = "gen/** linguist-generated\n".into();
  host.working = serde_json::from_value(serde_json::json!({
    "committed_files": [],
    "uncommitted_files": [
      {"path": "a.rs", "status": "M", "previous_path": null, "changed_line_count": 2, "diff_deferred": false},
      {"path": "Cargo.lock", "status": "M", "previous_path": null, "changed_line_count": 1, "diff_deferred": false},
      {"path": "gen/api.ts", "status": "A", "previous_path": null, "changed_line_count": 1, "diff_deferred": false}
    ],
    "hunks_by_file": [
      {"path": "a.rs", "hunks": [{"id": "h", "header": "@@ -1,2 +1,3 @@", "lines": [" a", "+b", "+c"], "patch": ""}]},
      {"path": "Cargo.lock", "hunks": [{"id": "h", "header": "@@ -1 +1 @@", "lines": ["-x", "+y"], "patch": ""}]},
      {"path": "gen/api.ts", "hunks": [{"id": "h", "header": "@@ -0,0 +1,1 @@", "lines": ["+z"], "patch": ""}]}
    ],
    "too_large_to_render": false
  }))
  .ok();
  host
}

#[gpui::test]
fn changes_tab_waits_to_read_generated_diffs(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host_with_generated(), changes_route());
  assert!(h.drawn("file-card-Cargo.lock"));
  assert!(h.drawn("load-generated-Cargo.lock"));
  assert!(h.drawn("load-generated-gen/api.ts"));
  assert!(!h.drawn("load-generated-a.rs"));
  let reads: Vec<String> = host
    .calls()
    .into_iter()
    .filter(|c| c.starts_with("read hunks"))
    .collect();
  assert_eq!(reads, vec!["read hunks [\"a.rs\"]".to_string()]);
}

#[gpui::test]
fn changes_tab_loads_a_generated_diff_on_request(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host_with_generated(), changes_route());
  h.click("load-generated-Cargo.lock");
  h.cx.run_until_parked();
  assert!(host
    .calls()
    .contains(&"read hunks [\"Cargo.lock\"]".to_string()));
  assert!(
    !h.drawn("load-generated-Cargo.lock"),
    "the diff shows instead"
  );
  assert!(h.drawn("load-generated-gen/api.ts"));
}

/// Clicks the filter button and `item` in its menu.
fn filter(h: &mut Harness, item: &str) {
  h.click("changes-filter");
  h.pick(item);
}

#[gpui::test]
fn changes_tab_hides_generated_files_from_the_list_and_diff(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host_with_generated(), changes_route());
  filter(&mut h, "Generated files");
  assert!(h.drawn("file-row-a.rs"));
  assert!(h.drawn("file-card-a.rs"));
  for hidden in [
    "file-row-Cargo.lock",
    "file-card-Cargo.lock",
    "file-row-gen/api.ts",
  ] {
    assert!(!h.drawn(hidden), "{hidden}");
  }
  filter(&mut h, "Generated files");
  assert!(h.drawn("file-row-Cargo.lock"));
  assert!(h.drawn("file-card-gen/api.ts"));
}

#[gpui::test]
fn changes_tab_keeps_a_selected_generated_file_listed(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host_with_generated(), changes_route());
  h.click("file-row-Cargo.lock");
  h.click("stage-Cargo.lock");
  filter(&mut h, "Generated files");
  assert!(h.drawn("unstage-Cargo.lock"));
  assert!(!h.drawn("file-row-gen/api.ts"));
}

#[gpui::test]
fn changes_tab_offers_the_filter_only_with_something_to_filter(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host(), changes_route());
  assert!(h.drawn("file-row-a.rs"));
  assert!(!h.drawn("changes-filter"));
}

fn committed_route(show: bool) -> ScreenSnapshot {
  let mut route = route_json(Some(7), "main");
  route["code"] = true.into();
  route["tab"] = "changes".into();
  route["showCommitted"] = show.into();
  serde_json::from_value(route).expect("changes route")
}

fn changes_host_with_committed() -> FakeHost {
  let mut host = changes_host();
  host.working = serde_json::from_value(serde_json::json!({
    "committed_files": [
      {"path": "c.rs", "status": "M", "previous_path": null, "changed_line_count": 1, "diff_deferred": false}
    ],
    "uncommitted_files": [
      {"path": "a.rs", "status": "M", "previous_path": null, "changed_line_count": 1, "diff_deferred": false}
    ],
    "hunks_by_file": [
      {"path": "a.rs", "hunks": [{"id": "h", "header": "@@ -1 +1 @@", "lines": ["-a", "+b"], "patch": ""}]},
      {"path": "c.rs", "hunks": [{"id": "h", "header": "@@ -1 +1 @@", "lines": ["-c", "+d"], "patch": ""}]}
    ],
    "too_large_to_render": false
  }))
  .ok();
  host
}

#[gpui::test]
fn changes_tab_filters_the_committed_group(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host_with_committed(), committed_route(true));
  assert!(h.drawn("committed-section-toggle"));
  assert!(h.drawn("file-card-c.rs"));
  filter(&mut h, "Committed");
  assert_eq!(
    fields(&h, "changes.showCommitted"),
    vec![serde_json::json!(false)]
  );
}

#[gpui::test]
fn changes_tab_hides_the_committed_group_when_filtered_out(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host_with_committed(), committed_route(false));
  assert!(h.drawn("file-row-a.rs"));
  assert!(h.drawn("changes-filter"));
  assert!(!h.drawn("committed-section-toggle"));
  assert!(!h.drawn("file-card-c.rs"));
}

#[gpui::test]
fn changes_tab_asks_for_a_commit_message(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  h.click("commit-message");
  h.cx.simulate_keystrokes("cmd-enter");
  h.cx.run_until_parked();
  assert!(h.drawn("commit-message-error"));
  assert!(!host.writes().iter().any(|w| w.starts_with("commit")));
}

#[gpui::test]
fn changes_tab_commits_the_working_copy(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  h.click("commit-message");
  h.cx.simulate_input("Fix");
  h.click("commit-button");
  h.cx.run_until_parked();
  assert!(
    host
      .writes()
      .contains(&"commit Some(7) \"Fix\"".to_string()),
    "{:?}",
    host.writes()
  );
}

#[gpui::test]
fn changes_tab_commits_only_staged_files(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  h.click("file-row-a.rs");
  h.click("stage-a.rs");
  h.cx.run_until_parked();
  assert!(h.drawn("unstage-a.rs"));
  h.click("commit-message");
  h.cx.simulate_input("Part");
  h.click("commit-button");
  h.cx.run_until_parked();
  assert!(
    host
      .writes()
      .contains(&"split \"Part\" [\"a.rs\"]".to_string()),
    "{:?}",
    host.writes()
  );
}

#[gpui::test]
fn changes_tab_marks_files_viewed(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  h.click("viewed-a.rs");
  h.cx.run_until_parked();
  assert!(
    host
      .writes()
      .iter()
      .any(|w| w.starts_with("mark viewed a.rs ")),
    "{:?}",
    host.writes()
  );
  h.click("viewed-a.rs");
  h.cx.run_until_parked();
  assert!(host.writes().contains(&"unmark viewed a.rs".to_string()));
}

#[gpui::test]
fn a_viewed_committed_file_stays_viewed_across_reloads(cx: &mut TestAppContext) {
  let mut host = changes_host();
  host.working = host.working.map(|mut diff| {
    let committed: crate::jj::JjRevisionDiff = serde_json::from_value(serde_json::json!({
      "committed_files": [
        {"path": "c.rs", "status": "M", "previous_path": null, "changed_line_count": 1, "diff_deferred": false}
      ],
      "hunks_by_file": [
        {"path": "c.rs", "hunks": [{"id": "h", "header": "@@ -1 +1 @@", "lines": ["-x", "+y"], "patch": ""}]}
      ],
      "too_large_to_render": false
    }))
    .expect("committed diff");
    diff.committed_files = committed.committed_files;
    diff.hunks_by_file.extend(committed.hunks_by_file);
    diff
  });
  let mut route = changes_route();
  if let View::Workspace(r) = &mut route.view {
    r.show_committed = Some(true);
  }
  let (mut h, host) = workspace_harness(cx, host, route);
  h.click("viewed-c.rs");
  h.cx.run_until_parked();
  edit_a_rs(&mut h, &host);
  let viewed = h.screen.read_with(&h.cx, |screen, cx| {
    let workspace = screen.workspace.as_ref().expect("workspace").read(cx);
    let changes = workspace.changes().expect("changes").read(cx);
    changes.viewed.contains_key("c.rs")
  });
  assert!(viewed, "{:?}", host.writes());
  assert!(!host.writes().contains(&"unmark viewed c.rs".to_string()));
}

#[gpui::test]
fn changes_tab_discards_everything_once_confirmed(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  h.click("discard-all-changes");
  assert!(h.drawn("discard-all-dialog"));
  h.click("discard-all-confirm");
  h.cx.run_until_parked();
  let writes = host.writes();
  // Snapshotted first, so the toast can undo it.
  assert_eq!(
    writes[..2],
    [
      "snapshot /repo/.treq/workspaces/login".to_string(),
      "discard /repo/.treq/workspaces/login".to_string()
    ],
    "{writes:?}"
  );
  assert!(!h.drawn("discard-all-dialog"));
}

/// Found by the fuzzer: the confirmation listens for Escape only while
/// focus is inside it, so opening it moves the focus there.
#[gpui::test]
fn escape_closes_the_discard_all_confirmation(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  h.click("discard-all-changes");
  assert!(h.drawn("discard-all-dialog"));
  h.cx.simulate_keystrokes("escape");
  assert!(!h.drawn("discard-all-dialog"));
  assert!(!host.writes().iter().any(|w| w.starts_with("discard")));
}

#[gpui::test]
fn changes_tab_reads_again_only_when_files_change(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  let reads = |host: &FakeHost| {
    host
      .calls()
      .iter()
      .filter(|c| c.starts_with("read workspace diff"))
      .count()
  };
  h.drawn("changes-diff-viewer");
  assert_eq!(reads(&host), 1);
  // React bumps this after each read the tab reports.
  h.update(|s| {
    if let View::Workspace(r) = &mut s.view {
      r.revision += 1;
    }
  });
  h.drawn("changes-diff-viewer");
  assert_eq!(reads(&host), 1);
  h.update(|s| {
    if let View::Workspace(r) = &mut s.view {
      r.changes_revision += 1;
    }
  });
  h.drawn("changes-diff-viewer");
  assert_eq!(reads(&host), 2);
}

/// An agent edits `a.rs` on disk: its `+c` becomes `+e`, and the watcher
/// bumps `changesRevision`.
fn edit_a_rs(h: &mut Harness, host: &FakeHost) {
  let mut edited = host.working.clone().expect("working");
  edited.hunks_by_file[0].hunks[0].lines = vec![" a".into(), "+b".into(), "+e".into()];
  *host.edited.lock().unwrap() = Some(edited);
  h.update(|s| {
    if let View::Workspace(r) = &mut s.view {
      r.changes_revision += 1;
    }
  });
  h.cx.run_until_parked();
}

/// The diff lines the Changes tab shows for `path`.
fn shown_lines(h: &mut Harness, path: &str) -> Vec<String> {
  h.cx.run_until_parked();
  h.screen.read_with(&h.cx, |screen, cx| {
    let workspace = screen.workspace.as_ref().expect("workspace").read(cx);
    let changes = workspace.changes().expect("changes").read(cx);
    changes.hunks[path]
      .hunks
      .iter()
      .flat_map(|h| h.lines.clone())
      .collect()
  })
}

#[gpui::test]
fn changes_tab_follows_edits_on_disk_outside_a_review(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  h.drawn("changes-diff-viewer");
  edit_a_rs(&mut h, &host);
  assert_eq!(shown_lines(&mut h, "a.rs"), [" a", "+b", "+e"]);
  assert!(!h.drawn("stale-banner"));
}

#[gpui::test]
fn a_refresh_that_reads_the_same_diff_keeps_its_rows(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  h.drawn("changes-diff-viewer");
  // The hunks `a.rs` shows and the rows' layout revision.
  let state = |h: &mut Harness| {
    h.drawn("changes-diff-viewer");
    h.screen.read_with(&h.cx, |screen, cx| {
      let workspace = screen.workspace.as_ref().expect("workspace").read(cx);
      let changes = workspace.changes().expect("changes").read(cx);
      (
        Arc::as_ptr(&changes.hunks["a.rs"].hunks),
        changes.layout_revision(),
      )
    })
  };
  let before = state(&mut h);
  h.update(|s| {
    if let View::Workspace(r) = &mut s.view {
      r.changes_revision += 1;
    }
  });
  assert_eq!(state(&mut h), before);
  edit_a_rs(&mut h, &host);
  let after = state(&mut h);
  assert_ne!(after.0, before.0, "the edited file is read again");
  assert_ne!(after.1, before.1, "and laid out again");
  assert_eq!(shown_lines(&mut h, "a.rs"), [" a", "+b", "+e"]);
}

#[gpui::test]
fn an_edit_on_disk_during_a_review_waits_behind_the_stale_banner(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  comment_line(&mut h, "Nit");
  edit_a_rs(&mut h, &host);
  assert!(h.drawn("stale-banner"), "a changed diff, same file list");
  assert_eq!(
    shown_lines(&mut h, "a.rs"),
    [" a", "+b", "+c"],
    "the reviewed diff stays"
  );
  h.click("reload-stale");
  assert!(!h.drawn("stale-banner"));
  assert_eq!(shown_lines(&mut h, "a.rs"), [" a", "+b", "+e"]);
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

#[gpui::test]
fn arrow_keys_move_and_extend_the_line_selection(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host(), changes_route());
  // a.rs: " a", "+b", "+c"; b.rs: "+d".
  h.click("gutter-a.rs-0-0");
  h.cx.simulate_keystrokes("down");
  assert_eq!(selected_lines(&mut h), Some(("a.rs".into(), vec![(0, 1)])));
  h.cx.simulate_keystrokes("shift-down");
  assert_eq!(
    selected_lines(&mut h),
    Some(("a.rs".into(), vec![(0, 1), (0, 2)]))
  );
  h.cx.simulate_keystrokes("shift-down");
  assert_eq!(
    selected_lines(&mut h),
    Some(("a.rs".into(), vec![(0, 1), (0, 2)])),
    "a selection stays in its file"
  );
  h.cx.simulate_keystrokes("down");
  assert_eq!(selected_lines(&mut h), Some(("b.rs".into(), vec![(0, 0)])));
  h.cx.simulate_keystrokes("up");
  assert_eq!(selected_lines(&mut h), Some(("a.rs".into(), vec![(0, 2)])));
  h.cx.simulate_keystrokes("shift-up");
  assert_eq!(
    selected_lines(&mut h),
    Some(("a.rs".into(), vec![(0, 1), (0, 2)]))
  );
  h.cx.simulate_keystrokes("escape");
  assert_eq!(selected_lines(&mut h), None);
}

#[gpui::test]
fn left_comments_the_selected_lines(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  h.click("gutter-a.rs-0-1");
  h.cx.simulate_keystrokes("shift-down left");
  assert!(h.drawn("comment-input"));
  h.cx.simulate_input("Both");
  h.cx.simulate_keystrokes("cmd-enter");
  settle_save(&mut h);
  let saved = host.review.lock().unwrap().clone().unwrap_or_default();
  assert!(
    saved.contains("\"start_line\":2") && saved.contains("\"end_line\":3"),
    "{saved}"
  );
  // The diff has the keyboard again.
  h.cx.simulate_keystrokes("down");
  assert!(selected_lines(&mut h).is_some());
}

#[gpui::test]
fn a_suggested_change_is_drawn_and_applied_to_the_file(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  h.click("gutter-a.rs-0-1");
  h.cx.simulate_keystrokes("shift-down left");
  h.click("suggest-change");
  assert_eq!(
    h.screen.read_with(&h.cx, |screen, cx| {
      let workspace = screen.workspace.as_ref().expect("workspace").read(cx);
      let changes = workspace.changes().expect("changes").read(cx);
      changes.review.draft_text.clone()
    }),
    "```suggestion\nb\nc\n```\n"
  );
  h.cx.simulate_keystrokes("cmd-enter");
  assert!(h.drawn("agent-review-suggestion"), "drawn as a diff");
  // The write changes a.rs on disk; that read is not stale.
  let mut applied = host.working.clone().expect("working");
  applied.hunks_by_file[0].hunks[0].lines = vec![" a".into(), "+B".into()];
  *host.edited.lock().unwrap() = Some(applied);
  h.click("apply-suggestion");
  h.cx.run_until_parked();
  assert!(!h.drawn("stale-banner"), "its own write shows at once");
  assert_eq!(shown_lines(&mut h, "a.rs"), [" a", "+B"]);
  assert!(
    host
      .calls()
      .iter()
      .any(|c| c.contains("apply suggestion") && c.contains("a.rs 2..3 [\"b\", \"c\"]")),
    "{:?}",
    host.calls()
  );
  assert!(
    !h.drawn("inline-comment-card"),
    "the applied comment is done"
  );
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

#[gpui::test]
fn changes_tab_comments_a_line_and_keeps_the_review(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  assert!(!h.drawn("review-action-bar"));
  comment_line(&mut h, "Nit");
  assert!(h.drawn("inline-comment-card"));
  assert!(h.drawn("review-action-bar"));
  settle_save(&mut h);
  assert!(host.writes().contains(&"save review 7 None".to_string()));
  let saved = host.review.lock().unwrap().clone().unwrap_or_default();
  assert!(saved.contains("\"Nit\""), "{saved}");
  assert!(saved.contains("\"file_path\":\"a.rs\""), "{saved}");
  h.click("delete-comment");
  h.cx.run_until_parked();
  assert!(!h.drawn("inline-comment-card"));
  assert!(!h.drawn("review-action-bar"));
  assert!(host.writes().contains(&"clear review 7".to_string()));
}

#[gpui::test]
fn changes_tab_edits_a_comment(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  comment_line(&mut h, "Nit");
  h.click("inline-comment-card");
  assert!(h.drawn("comment-save"));
  h.click("comment-edit");
  h.cx.simulate_input("!");
  h.click("comment-save");
  settle_save(&mut h);
  let saved = host.review.lock().unwrap().clone().unwrap_or_default();
  assert!(saved.contains("\"Nit!\""), "{saved}");
}

#[gpui::test]
fn changes_tab_comments_a_file(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host(), changes_route());
  h.click("add-file-comment-button");
  assert!(h.drawn("comment-input"));
  h.click("comment-input");
  h.cx.simulate_input("Split this");
  h.click("comment-submit");
  assert!(h.drawn("file-comment-card"));
  assert!(h.drawn("review-action-bar"));
}

#[gpui::test]
fn changes_tab_reads_the_kept_review(cx: &mut TestAppContext) {
  let host = changes_host();
  *host.review.lock().unwrap() = Some(
    serde_json::json!([{
      "id": "c1", "file_path": "a.rs", "hunk_id": "h", "start_line": 1, "end_line": 1,
      "line_content": ["+b"], "text": "Kept", "created_at": "", "side": "new"
    }])
    .to_string(),
  );
  let (mut h, host) = workspace_harness(cx, host, changes_route());
  assert!(host.calls().contains(&"read review 7".to_string()));
  assert!(h.drawn("inline-comment-card"));
  assert!(h.drawn("review-action-bar"));
}

#[gpui::test]
fn changes_tab_sends_the_review_to_an_agent(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  comment_line(&mut h, "Nit");
  h.click("finish-review");
  assert!(h.drawn("finish-review-popover"));
  h.click("review-plan");
  h.cx.run_until_parked();
  assert!(fields(&h, "changes.review").is_empty());
  let launches = host.launches.lock().unwrap().clone();
  assert_eq!(launches.len(), 1);
  let (request, select) = &launches[0];
  assert_eq!(request.session_name.as_deref(), Some("Code Review"));
  assert_eq!(request.mode.as_deref(), Some("plan"));
  assert_eq!(request.workspace_id, Some(7));
  assert!(!select);
  let markdown = request.prompt.clone().unwrap_or_default();
  assert!(
    markdown.contains("Nit") && markdown.contains("a.rs"),
    "{markdown}"
  );
  assert!(!h.drawn("review-action-bar"));
  assert!(!h.drawn("finish-review-popover"));
  assert!(host.writes().contains(&"clear review 7".to_string()));
}

#[gpui::test]
fn changes_tab_starts_an_ai_review_in_a_new_terminal(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  h.click("start-agent-review");
  h.cx.run_until_parked();
  assert!(fields(&h, "changes.startReview").is_empty());
  let launches = host.launches.lock().unwrap().clone();
  assert_eq!(launches.len(), 1, "{:?}", host.calls());
  let (request, select) = &launches[0];
  assert_eq!(request.session_name.as_deref(), Some("AI Review"));
  assert_eq!(request.mode.as_deref(), Some("acceptEdits"));
  assert_eq!(request.workspace_id, Some(7));
  assert!(!select);
  let prompt = request.prompt.clone().unwrap_or_default();
  assert!(prompt.contains("--target-type workspace_diff"), "{prompt}");
  assert!(prompt.contains("--target-id 7"), "{prompt}");
}

#[gpui::test]
fn changes_tab_discards_the_review_once_confirmed(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host(), changes_route());
  comment_line(&mut h, "Nit");
  h.click("discard-review");
  assert!(h.drawn("discard-review-dialog"));
  h.click("confirm-discard-review");
  assert!(!h.drawn("discard-review-dialog"));
  assert!(!h.drawn("review-action-bar"));
  assert!(!h.drawn("inline-comment-card"));
}

#[gpui::test]
fn changes_tab_comments_a_dragged_selection(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  h.cx.run_until_parked();
  let from = h
    .cx
    .debug_bounds("gutter-a.rs-0-1")
    .expect("gutter")
    .center();
  let to = h
    .cx
    .debug_bounds("gutter-a.rs-0-2")
    .expect("gutter")
    .center();
  h.cx
    .simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
  h.cx
    .simulate_mouse_move(to, Some(MouseButton::Left), Modifiers::none());
  h.cx
    .simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
  h.click("add-comment-a.rs-0-2");
  h.click("comment-input");
  h.cx.simulate_input("Both");
  h.click("comment-submit");
  settle_save(&mut h);
  let saved: serde_json::Value =
    serde_json::from_str(&host.review.lock().unwrap().clone().unwrap_or_default())
      .expect("saved review");
  assert_eq!(saved[0]["start_line"], 2, "{saved}");
  assert_eq!(saved[0]["end_line"], 3, "{saved}");
  assert_eq!(saved[0]["line_content"], serde_json::json!(["+b", "+c"]));
}

#[gpui::test]
fn changes_tab_finds_text_in_the_diff(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host(), changes_route());
  h.click("file-card-a.rs");
  h.cx.simulate_keystrokes("cmd-f");
  assert!(h.drawn("diff-search"));
  // a.rs: " a", "+b", "+c"; b.rs: "+d".
  h.cx.simulate_input("b");
  h.cx
    .executor()
    .advance_clock(std::time::Duration::from_millis(150));
  h.cx.run_until_parked();
  let count = |h: &mut Harness| {
    h.cx.run_until_parked();
    h.screen.read_with(&h.cx, |screen, cx| {
      let workspace = screen.workspace.as_ref().expect("workspace").read(cx);
      let search = &workspace.changes().expect("changes").read(cx).search;
      (search.current, search.total)
    })
  };
  assert_eq!(count(&mut h), (0, 1));
  h.cx.simulate_keystrokes("enter");
  assert_eq!(count(&mut h), (0, 1));
  h.cx.simulate_keystrokes("escape");
  assert!(!h.drawn("diff-search"));
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

fn agent_comment(suggested: Option<&str>) -> crate::local_db::AgentReviewComment {
  serde_json::from_value(serde_json::json!({
    "id": "ag", "repo_path": "/repo", "target_type": "workspace_diff", "target_id": "7",
    "file_path": "a.rs", "hunk_id": null, "start_line": 2, "end_line": 2, "side": "new",
    "comment_text": "Use one line", "suggested_replacement": suggested, "status": "open",
    "source": "local-agent", "created_at": "", "resolved_at": null
  }))
  .expect("agent comment")
}

#[gpui::test]
fn changes_tab_shows_review_threads_on_their_line(cx: &mut TestAppContext) {
  let mut host = changes_host();
  host.threads = vec![gh_thread("t1", 2, false), gh_thread("t2", 40, true)];
  let (mut h, host) = workspace_harness(cx, host, changes_route());
  assert!(
    host
      .calls()
      .contains(&"read threads feature/login".to_string()),
    "{:?}",
    host.calls()
  );
  // Line 2 is in the diff; line 40 is not, so it sits above the file.
  assert!(h.drawn("github-comment-card"));
  assert!(h.drawn("github-outdated-threads"));
  // The resolved one starts folded.
  assert!(h.drawn("github-thread-collapsed-preview"));
}

#[gpui::test]
fn changes_tab_quotes_a_review_thread(cx: &mut TestAppContext) {
  let mut host = changes_host();
  host.threads = vec![gh_thread("t1", 2, false)];
  let (mut h, host) = workspace_harness(cx, host, changes_route());
  h.click("github-comment-quote");
  assert!(h.drawn("comment-input"));
  h.click("comment-input");
  h.cx.simulate_input("Agreed");
  h.click("comment-submit");
  settle_save(&mut h);
  let saved: serde_json::Value =
    serde_json::from_str(&host.review.lock().unwrap().clone().unwrap_or_default())
      .expect("saved review");
  assert_eq!(saved[0]["source"], "github", "{saved}");
  assert_eq!(saved[0]["github_author"], "octo");
  assert_eq!(
    saved[0]["line_content"],
    serde_json::json!(["Why two lines?"])
  );
  assert_eq!(saved[0]["start_line"], 2);
}

#[gpui::test]
fn changes_tab_reacts_to_a_review_comment(cx: &mut TestAppContext) {
  let mut host = changes_host();
  host.threads = vec![gh_thread("t1", 2, false)];
  let (mut h, host) = workspace_harness(cx, host, changes_route());
  assert!(!h.drawn("reaction-t1-c-EYES"));
  h.click("reaction-add-t1-c");
  h.click("reaction-pick-t1-c-EYES");
  h.cx.run_until_parked();
  assert!(h.drawn("reaction-t1-c-EYES"));
  assert!(host.writes().contains(&"react t1-c EYES true".to_string()));
  h.click("reaction-t1-c-EYES");
  h.cx.run_until_parked();
  assert!(!h.drawn("reaction-t1-c-EYES"));
  assert!(host.writes().contains(&"react t1-c EYES false".to_string()));
}

#[gpui::test]
fn changes_tab_resolves_an_agent_comment(cx: &mut TestAppContext) {
  let host = changes_host();
  *host.agent.lock().unwrap() = vec![agent_comment(Some("b and c"))];
  let (mut h, host) = workspace_harness(cx, host, changes_route());
  assert!(h.drawn("agent-review-comment-card"));
  assert!(h.drawn("agent-review-suggestion"));
  h.click("agent-resolve-ag");
  h.cx.run_until_parked();
  assert!(host
    .writes()
    .contains(&"resolve agent comment ag".to_string()));
  assert!(!h.drawn("agent-review-comment-card"));
}

#[gpui::test]
fn changes_tab_counts_open_agent_comments(cx: &mut TestAppContext) {
  let host = changes_host();
  *host.agent.lock().unwrap() = vec![agent_comment(None)];
  let (mut h, _) = workspace_harness(cx, host, changes_route());
  assert!(h.drawn("agent-review-comment-count"));
  h.click("agent-resolve-ag");
  h.cx.run_until_parked();
  assert!(!h.drawn("agent-review-comment-count"));
}

#[gpui::test]
fn changes_tab_applies_an_agent_suggestion_and_reads_again(cx: &mut TestAppContext) {
  let host = changes_host();
  *host.agent.lock().unwrap() = vec![agent_comment(Some("b and c"))];
  let (mut h, host) = workspace_harness(cx, host, changes_route());
  let reads = |host: &FakeHost| {
    host
      .calls()
      .iter()
      .filter(|c| c.starts_with("read workspace diff"))
      .count()
  };
  h.drawn("agent-review-comment-card");
  assert_eq!(reads(&host), 1);
  h.click("agent-apply-ag");
  h.cx.run_until_parked();
  assert!(host
    .writes()
    .contains(&"apply agent suggestion ag".to_string()));
  assert_eq!(reads(&host), 2);
}

/// The Changes tab with GPUI's terminal pane below, its agent session 3
/// open on `feature/login`.
fn changes_with_terminal(cx: &mut TestAppContext) -> (Harness, Arc<FakeHost>) {
  let mut host = changes_host();
  host.sessions = terminal_host().sessions;
  host.workspaces = host.loaded.workspace.clone().into_iter().collect();
  *host.agent.lock().unwrap() = vec![agent_comment(Some("one()"))];
  let mut route = route_json(Some(7), "main");
  route["code"] = true.into();
  route["tab"] = "changes".into();
  route["terminal"] = serde_json::json!({ "activeSessionId": 3 });
  let route = serde_json::from_value(route).expect("changes route");
  workspace_harness(cx, host, route)
}

#[gpui::test]
fn changes_tab_sends_an_agent_comment_to_the_idle_agent(cx: &mut TestAppContext) {
  let (mut h, host) = changes_with_terminal(cx);
  assert!(h.drawn("terminal-column-agent"));
  h.click("agent-send-ag");
  h.cx.run_until_parked();
  assert!(fields(&h, "changes.agentComment").is_empty());
  let prompt = "Please address this review comment.\n\na.rs:2\n> Use one line\n\nSuggested change:\n```\none()\n```\n";
  let sent = format!("write {} {:?}", agent_pty(), format!("{prompt}\n"));
  assert!(pty_writes(&host).contains(&sent), "{:?}", pty_writes(&host));
  assert!(host.launches.lock().unwrap().is_empty());
  assert!(host
    .calls()
    .contains(&"notify toast Comment sent to agent".to_string()));
}

#[gpui::test]
fn changes_tab_starts_an_agent_for_a_comment_without_an_idle_one(cx: &mut TestAppContext) {
  // The open agent runs in another workspace, so it is not this one's.
  let mut sessions = terminal_host().sessions;
  sessions[0].workspace_id = Some(99);
  let mut changes = changes_host();
  changes.sessions = sessions;
  changes.workspaces = changes.loaded.workspace.clone().into_iter().collect();
  *changes.agent.lock().unwrap() = vec![agent_comment(None)];
  let mut route = route_json(Some(7), "main");
  route["code"] = true.into();
  route["tab"] = "changes".into();
  route["terminal"] = serde_json::json!({ "activeSessionId": 3 });
  let (mut h, host) = workspace_harness(cx, changes, serde_json::from_value(route).expect("route"));
  assert!(h.drawn("terminal-column-agent"));
  h.click("agent-send-ag");
  h.cx.run_until_parked();
  let launches = host.launches.lock().unwrap().clone();
  assert_eq!(launches.len(), 1, "{:?}", host.calls());
  let request = &launches[0].0;
  assert_eq!(request.session_name.as_deref(), Some("Review Comment"));
  assert_eq!(request.mode.as_deref(), Some("acceptEdits"));
  assert_eq!(
    request.prompt.as_deref(),
    Some("Please address this review comment.\n\na.rs:2\n> Use one line\n")
  );
  assert!(host
    .calls()
    .contains(&"notify toast Comment sent to agent".to_string()));
}

#[gpui::test]
fn changes_tab_sends_an_agent_comment_to_react(cx: &mut TestAppContext) {
  let host = changes_host();
  *host.agent.lock().unwrap() = vec![agent_comment(None)];
  let (mut h, _) = workspace_harness(cx, host, changes_route());
  assert!(!h.drawn("agent-apply-ag"));
  h.click("agent-send-ag");
  h.cx.run_until_parked();
  let sent = fields(&h, "changes.agentComment");
  assert_eq!(sent.len(), 1);
  assert_eq!(sent[0]["id"], "ag");
  assert_eq!(sent[0]["comment_text"], "Use one line");
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

#[gpui::test]
fn logs_tab_lists_the_check_logs(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, logs_host(), logs_route(None));
  for selector in [
    "logs-tab",
    "repo-logs-output",
    "repo-log-line",
    "log-level-filter",
  ] {
    assert!(h.drawn(selector), "{selector}");
  }
  assert!(
    host
      .calls()
      .contains(&"read check logs None None Some([])".to_string()),
    "{:?}",
    host.calls()
  );
}

#[gpui::test]
fn logs_tab_sends_a_picked_line_to_an_agent(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, logs_host(), logs_route(None));
  assert!(!h.drawn("log-inspect-panel"));
  h.click("repo-log-line");
  assert!(h.drawn("log-inspect-panel"));
  assert!(h.drawn("selection-count"));
  h.click("send-to-agent");
  h.cx.run_until_parked();
  let launches = host.launches.lock().unwrap().clone();
  assert_eq!(launches.len(), 1);
  assert_eq!(launches[0].0.session_name.as_deref(), Some("Logs"));
  assert_eq!(launches[0].0.mode.as_deref(), Some("plan"));
  let prompt = launches[0].0.prompt.clone().unwrap_or_default();
  assert!(
    prompt.starts_with("Here are 1 log line from my check logs."),
    "{prompt}"
  );
  assert!(prompt.contains("  build/Test  "), "{prompt}");
}

#[gpui::test]
fn logs_tab_opens_on_the_source_react_asks_for(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, logs_host(), logs_route(Some("system")));
  assert!(h.drawn("system-logs-output"));
  h.click("open-log-folder");
  h.cx.run_until_parked();
  assert!(host.writes().contains(&"open log folder".to_string()));
}

#[gpui::test]
fn logs_tab_switches_source_from_its_menu(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, logs_host(), logs_route(None));
  assert!(h.drawn("repo-logs-output"));
  h.click("log-source");
  h.pick("System");
  assert!(h.drawn("system-logs-output"));
  assert!(host.calls().contains(&"read system logs None".to_string()));
}

#[gpui::test]
fn logs_tab_shows_an_agent_chat(cx: &mut TestAppContext) {
  use crate::core::agent_chat::{AgentChat, AgentType, ChatMessage, ChatRole};
  let mut host = logs_host();
  host.chats = vec![AgentChat {
    session_id: 4,
    pty_session_id: "p".into(),
    name: "Fix login".into(),
    agent: AgentType::Claude,
    workspace_id: Some(7),
    created_at: String::new(),
    screen_before_last_user_message: String::new(),
    messages: vec![ChatMessage {
      id: 1,
      role: ChatRole::User,
      message: "Why does login fail?".into(),
      time: "2026-10-19T10:00:00Z".into(),
    }],
  }];
  let (mut h, host) = workspace_harness(cx, host, logs_route(Some("agent-chats")));
  assert!(h.drawn("agent-chat-output"));
  assert!(h.drawn("agent-chat-line"));
  assert!(host.calls().contains(&"read agent chat 4".to_string()));
}

#[gpui::test]
fn logs_tab_runs_a_query_in_the_explorer(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, logs_host(), logs_route(None));
  h.click("logs-explorer-view");
  assert!(h.drawn("logs-sql-explorer"));
  assert!(h.drawn("sql-editor"));
  h.click("run-query");
  h.cx.run_until_parked();
  assert!(
    host.calls().contains(
      &"run sql SELECT timestamp, severityText, json_extract(attributes, '$.job_id') AS job_id,"
        .to_string()
    ),
    "{:?}",
    host.calls()
  );
  assert!(h.drawn("sql-error"));
  assert!(!h.drawn("send-resultset-to-agent"));
}

#[gpui::test]
fn logs_tab_sends_a_result_set_to_an_agent(cx: &mut TestAppContext) {
  let mut host = logs_host();
  host.sql = Some(Ok(crate::core::checks_logs::SqlResult {
    columns: vec!["job_id".into()],
    rows: vec![vec![Some("build".into())]],
    row_count: 1,
  }));
  let (mut h, host) = workspace_harness(cx, host, logs_route(None));
  h.click("logs-explorer-view");
  h.click("run-query");
  h.cx.run_until_parked();
  assert!(h.drawn("sql-results"));
  h.click("send-resultset-to-agent");
  h.cx.run_until_parked();
  let launches = host.launches.lock().unwrap().clone();
  assert_eq!(launches.len(), 1);
  let prompt = launches[0].0.prompt.clone().unwrap_or_default();
  assert!(prompt.contains("Result (1 row):"), "{prompt}");
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

#[gpui::test]
fn terminal_pane_hides_its_chip_without_terminals(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  assert!(!h.drawn("terminal-pane-controls"));
  assert!(!h.drawn("terminal-scroll-container"));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  assert!(h.drawn("terminal-scroll-container"));
  h.click("terminal-collapse");
  assert!(!h.drawn("terminal-scroll-container"));
  h.click("terminal-expand");
  assert!(h.drawn("terminal-scroll-container"));
}

#[gpui::test]
fn terminal_pane_opens_the_active_agent_session(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  // The backend kept what the session starts with.
  host.pending.lock().unwrap().insert(
    3,
    launch_data::PendingSession {
      pending_prompt: Some("fix it".into()),
      permission_mode: Some("plan".into()),
      agent: Some("codex".into()),
      workspace_path: None,
    },
  );
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  assert!(h.drawn("terminal-column-agent"));
  assert!(h.drawn("terminal-column-active"));
  assert!(h.drawn("terminal-workspace-label"));
  let pty = agent_pty();
  let calls = host.calls();
  assert!(
    calls.contains(&format!("register chat 3 {pty} Claude 1")),
    "{calls:?}"
  );
  assert!(
    calls.contains(&"prepare codex Some(\"fix it\")".to_string()),
    "{calls:?}"
  );
  assert!(
    calls.contains(&format!("spawn {pty} in None running Some(\"codex\")")),
    "{calls:?}"
  );
  let summaries = h.sent().into_iter().find_map(|a| match a {
    ScreenAction::Field { field, value } if field == "terminal.summaries" => Some(value),
    _ => None,
  });
  assert_eq!(
    summaries,
    Some(serde_json::json!([{
      "id": "agent-3",
      "kind": "agent",
      "name": "Claude 1",
      "branchName": "feature/login",
      "repoPath": "/repo",
      "isMainRepo": false,
      "agent": "codex",
      "isStreaming": false,
    }]))
  );
}

/// A supporting repository on screen: the backend numbers its sessions
/// relative to the window's main repository, so the pane must too.
#[gpui::test]
fn terminal_pane_opens_an_agent_session_of_a_supporting_repo(cx: &mut TestAppContext) {
  let host = FakeHost {
    supporting: vec!["/support".into()],
    ..terminal_host()
  };
  let mut route = route_json(Some(7), "main");
  route["repoPath"] = "/support".into();
  route["code"] = true.into();
  route["terminal"] = serde_json::json!({});
  let route = serde_json::from_value(route).expect("terminal route");
  let (mut h, host) = workspace_harness(cx, host, route);
  let pane_id = terminal_data::pane_session_id("/support", "/repo", 3);
  set_terminal(
    &mut h,
    serde_json::json!({ "activeSessionId": pane_id, "mainRepoPath": "/repo" }),
  );
  assert!(h.drawn("terminal-column-agent"));
  let pty = terminal_data::agent_pty_id("/support", 3);
  let calls = host.calls();
  assert!(
    calls
      .iter()
      .any(|c| c.starts_with(&format!("spawn {pty} "))),
    "{calls:?}"
  );
}

#[gpui::test]
fn terminal_draws_output_and_types_into_the_pty(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  let pty = agent_pty();
  host.output(&pty, "hello \x1b[32mworld\x1b[0m\r\n$ ");
  h.cx.run_until_parked();
  let host_dyn: terminal_data::Host = host.clone();
  let (live, fresh) = terminal_data::attach(&host_dyn, &pty, 80, 24);
  assert!(!fresh);
  assert_eq!(live.screen_text(), "hello world\n$");
  h.click("terminal-body");
  h.cx.simulate_input("ls");
  h.cx.simulate_keystrokes("enter ctrl-c");
  let writes: Vec<String> = host
    .calls()
    .into_iter()
    .filter(|c| c.starts_with("write "))
    .collect();
  assert_eq!(
    writes,
    vec![
      format!("write {pty} \"l\""),
      format!("write {pty} \"s\""),
      format!("write {pty} \"\\r\""),
      format!("write {pty} \"\\u{{3}}\""),
    ]
  );
}

/// A terminal resized while its session starts hands the PTY the new size
/// once it runs: a program drawing for a wider PTY than the grid wraps
/// every line, so redraws stack up and overwrite each other.
#[gpui::test]
fn terminal_pty_follows_a_resize_while_it_starts(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  let pty = agent_pty();
  h.update(|s| {
    if let View::Workspace(route) = &mut s.view {
      route.terminal =
        Some(serde_json::from_value(serde_json::json!({ "activeSessionId": 3 })).expect("route"));
    }
  });
  assert!(
    !host.calls().iter().any(|c| c.starts_with("spawn ")),
    "the session is still starting"
  );
  h.cx
    .simulate_resize(gpui::size(gpui::px(900.), gpui::px(900.)));
  h.cx.run_until_parked();
  let host_dyn: terminal_data::Host = host.clone();
  let (live, _) = terminal_data::attach(&host_dyn, &pty, 80, 24);
  let grid = live.emulator.lock().unwrap().size();
  let last = host
    .resizes
    .lock()
    .unwrap()
    .iter()
    .rev()
    .find(|(id, ..)| *id == pty)
    .map(|(_, cols, rows)| (*cols as usize, *rows as usize));
  assert_eq!(last, Some((grid.cols, grid.rows)));
}

#[gpui::test]
fn terminal_search_keeps_what_is_typed_out_of_the_pty(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  h.click("terminal-body");
  h.click("terminal-search-button");
  assert!(h.drawn("terminal-search"));
  h.cx.simulate_input("dark");
  h.cx.run_until_parked();
  let writes: Vec<String> = host
    .calls()
    .into_iter()
    .filter(|c| c.starts_with("write "))
    .collect();
  assert_eq!(writes, Vec::<String>::new());
}

#[gpui::test]
fn terminal_pane_drops_the_active_column_on_a_click_outside(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  assert!(h.drawn("terminal-column-active"));
  h.click("terminal-maximize");
  // The pane's controls are part of it.
  assert!(h.drawn("terminal-column-active"));
  h.click("terminal-restore");
  h.click("workspace-header");
  assert!(!h.drawn("terminal-column-active"));
}

#[gpui::test]
fn terminal_pane_picks_the_environment_new_terminals_run_in(cx: &mut TestAppContext) {
  let mut host = terminal_host();
  host.devcontainers = true;
  let (mut h, host) = workspace_harness(cx, host, terminal_route(serde_json::json!({})));
  assert!(!h.drawn("terminal-environment"));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  h.click("terminal-environment");
  assert!(h.drawn("terminal-environment-menu"));
  h.click("environment-local");
  assert!(!h.drawn("terminal-environment-menu"));
  assert!(
    host
      .calls()
      .contains(&"set repo /repo environment=local".to_string()),
    "{:?}",
    host.calls()
  );
}

#[gpui::test]
fn terminal_pane_opens_and_closes_shells_by_shortcut(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  h.click("terminal-body");
  h.cx.simulate_keystrokes("cmd-\\");
  h.cx.run_until_parked();
  assert!(h.drawn("terminal-column-shell"));
  let spawned_shell = host.calls().iter().any(|c| {
    c.starts_with("spawn shell--repo--treq-workspaces-login-")
      && c.contains("Some(\"/repo/.treq/workspaces/login\") running Some(\"export\")")
  });
  assert!(spawned_shell, "{:?}", host.calls());
  // The new shell has focus: Cmd+W closes it, and the agent next to it
  // takes focus.
  h.cx.simulate_keystrokes("cmd-w");
  h.cx.run_until_parked();
  assert!(!h.drawn("terminal-column-shell"));
  assert!(h.drawn("terminal-column-agent"));
  h.cx.simulate_keystrokes("cmd-w");
  h.cx.run_until_parked();
  assert!(!h.drawn("terminal-column-agent"));
  assert!(host.calls().contains(&format!("close {}", agent_pty())));
  assert!(h.sent().contains(&field("terminal.closeSession", 3.into())));
  // Closing the last terminal hides the pane, chip included.
  assert!(!h.drawn("terminal-pane-controls"));
}

#[gpui::test]
fn terminal_pane_keeps_its_terminals_across_settings(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  h.click("terminal-body");
  h.cx.simulate_keystrokes("cmd-\\");
  assert!(h.drawn("terminal-column-shell"));
  let workspace = h.screen.read_with(&h.cx, |s, _| s.snapshot().clone());
  let spawns = |host: &FakeHost| {
    host
      .calls()
      .iter()
      .filter(|c| c.starts_with("spawn "))
      .count()
  };
  let spawned = spawns(&host);

  h.update(|s| *s = settings());
  h.cx.run_until_parked();
  assert!(!h.drawn("workspace-terminal-pane"));
  // The hidden pane takes no shortcuts.
  h.cx.simulate_keystrokes("cmd-\\");
  h.update(|s| *s = workspace);
  h.cx.run_until_parked();

  assert!(h.drawn("terminal-column-agent"));
  assert!(h.drawn("terminal-column-shell"));
  assert_eq!(spawns(&host), spawned, "{:?}", host.calls());
}

#[gpui::test]
fn terminal_pane_shows_a_closed_windows_terminals_again(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  h.click("terminal-body");
  h.cx.simulate_keystrokes("cmd-\\");
  assert!(h.drawn("terminal-column-shell"));
  let spawns = |host: &FakeHost| {
    host
      .calls()
      .iter()
      .filter(|c| c.starts_with("spawn "))
      .count()
  };
  let spawned = spawns(&host);
  h.cx.update(|window, _| window.remove_window());
  h.cx.run_until_parked();
  drop(h);

  // Another window opens on the repository: its pane shows them, running.
  let mut other = Harness::with_host(cx, terminal_route(serde_json::json!({})), host.clone());
  other
    .cx
    .simulate_resize(gpui::size(gpui::px(1400.), gpui::px(900.)));
  other.cx.run_until_parked();
  assert!(other.drawn("terminal-column-agent"));
  assert!(other.drawn("terminal-column-shell"));
  assert_eq!(spawns(&host), spawned, "{:?}", host.calls());

  // A third window on it finds none left to show.
  let mut third = Harness::with_host(cx, terminal_route(serde_json::json!({})), host);
  third.cx.run_until_parked();
  assert!(!third.drawn("terminal-column-shell"));
}

#[gpui::test]
fn terminal_pane_asks_react_for_a_new_agent(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  h.click("terminal-body");
  h.cx.simulate_keystrokes("cmd-]");
  h.cx.run_until_parked();
  assert!(h.sent().contains(&field(
    "terminal.newAgent",
    "/repo/.treq/workspaces/login".into()
  )));
}

#[gpui::test]
fn terminal_pane_runs_dashboard_commands(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  set_terminal(
    &mut h,
    serde_json::json!({
      "activeSessionId": 3,
      "commands": [{ "nonce": 1, "kind": "send", "sessionId": 3, "text": "review this" }],
    }),
  );
  assert!(host
    .calls()
    .contains(&format!("write {} \"review this\\n\"", agent_pty())));
  set_terminal(
    &mut h,
    serde_json::json!({
      "activeSessionId": 3,
      "commands": [{ "nonce": 2, "kind": "toggleMaximize" }],
    }),
  );
  assert!(h.drawn("terminal-restore"));
  assert!(!h.drawn("code-readme"), "the maximized pane covers the tab");
  assert!(h.drawn("workspace-header"), "the header stays above it");
  set_terminal(
    &mut h,
    serde_json::json!({
      "activeSessionId": 3,
      "commands": [{ "nonce": 3, "kind": "toggleCollapse" }],
    }),
  );
  assert!(h.drawn("terminal-expand"));
}

#[gpui::test]
fn terminal_pane_activates_the_selected_workspaces_terminal(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(
    &mut h,
    serde_json::json!({
      "activeSessionId": 3,
      "commands": [{ "nonce": 1, "kind": "newShell", "dir": "/repo" }],
    }),
  );
  // Selecting the home repository makes its shell the active terminal.
  h.update(|s| {
    if let View::Workspace(route) = &mut s.view {
      route.workspace_id = None;
    }
  });
  h.cx.run_until_parked();
  h.cx.simulate_keystrokes("cmd-w");
  h.cx.run_until_parked();
  assert!(!h.drawn("terminal-column-shell"));
  assert!(h.drawn("terminal-column-agent"));
  assert!(!host.calls().contains(&format!("close {}", agent_pty())));
}

fn open_remote_command(nonce: u64) -> serde_json::Value {
  serde_json::json!({
    "commands": [{
      "nonce": nonce,
      "kind": "openRemote",
      "remote": {
        "endpoint": crate::gpui_shell::remote_repo::tests::repository().endpoint,
        "repositoryId": "/srv/app",
        "workspaceId": "root",
        "remoteWorkingDirectory": "/srv/app",
        "label": "shell-k1",
        "launch": { "type": "shell" },
        "reattach": false,
      },
    }],
  })
}

#[gpui::test]
fn terminal_pane_attaches_a_remote_session(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, open_remote_command(1));
  assert!(h.drawn("terminal-column-shell"));
  assert!(h.drawn("remote-terminal-header"));
  assert!(h.drawn("remote-terminal-stop"));
  assert!(h.drawn("remote-terminal-detach"));
  let attached = host.attached();
  assert_eq!(attached.len(), 1, "{:?}", host.calls());
  assert!(attached[0].starts_with("remote-pty-ep-1-root-shell-k1-"));
  assert!(!h.drawn("remote-terminal-attaching"), "attached");
  h.cx.simulate_input("ls");
  h.cx.run_until_parked();
  assert!(host
    .calls()
    .iter()
    .any(|c| c.starts_with(&format!("write {}", attached[0]))));
  // The session ended on the host: the header says so, no overlay.
  *host.remote_running.lock().unwrap() = Some(false);
  host.exit(&attached[0], Some(0));
  h.cx.run_until_parked();
  assert!(host.calls().contains(&"list shell-k1".to_string()));
  assert!(h.drawn("remote-terminal-ended"));
  assert!(!h.drawn("remote-terminal-lost"));
}

#[gpui::test]
fn a_closed_windows_remote_session_shows_again(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, open_remote_command(1));
  let attached = host.attached();
  h.cx.update(|window, _| window.remove_window());
  h.cx.run_until_parked();
  drop(h);

  // Its channel still open, the next window draws it without attaching.
  let mut other = Harness::with_host(cx, terminal_route(serde_json::json!({})), host.clone());
  other
    .cx
    .simulate_resize(gpui::size(gpui::px(1400.), gpui::px(900.)));
  other.cx.run_until_parked();
  assert!(other.drawn("remote-terminal-header"));
  assert_eq!(host.attached(), attached, "{:?}", host.calls());
  // Its channel dropped since: the column attaches to the host's session.
  other.cx.update(|window, _| window.remove_window());
  other.cx.run_until_parked();
  drop(other);
  host.ptys.lock().unwrap().clear();
  let mut third = Harness::with_host(cx, terminal_route(serde_json::json!({})), host.clone());
  third
    .cx
    .simulate_resize(gpui::size(gpui::px(1400.), gpui::px(900.)));
  third.cx.run_until_parked();
  assert!(third.drawn("remote-terminal-header"));
  assert_eq!(
    host.attached(),
    vec![attached[0].clone(), attached[0].clone()]
  );
}

#[gpui::test]
fn a_dropped_remote_session_reattaches_on_every_drop(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, open_remote_command(1));
  *host.remote_running.lock().unwrap() = Some(true);
  host.exit(&host.attached()[0], None);
  h.cx.run_until_parked();
  let attached = host.attached();
  assert_eq!(attached.len(), 2, "reattached on its own");
  assert_ne!(attached[0], attached[1], "with a fresh local id");
  assert!(host.calls().contains(&format!("close {}", attached[0])));
  // Once the session talks again, a later drop gets its own attempt.
  host.output(&attached[1], "$ ");
  h.cx.run_until_parked();
  host.exit(&attached[1], None);
  h.cx.run_until_parked();
  assert_eq!(host.attached().len(), 3);
  assert!(!h.drawn("remote-terminal-lost"));
}

#[gpui::test]
fn a_lost_host_is_retried_once_before_offering_reattach(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, open_remote_command(1));
  // The host is unreachable: `PtyList` fails and so does the attach.
  *host.remote_running.lock().unwrap() = None;
  *host.attach_down.lock().unwrap() = true;
  host.exit(&host.attached()[0], None);
  h.cx.run_until_parked();
  assert_eq!(host.attached().len(), 2, "tried once on its own");
  assert!(h.drawn("remote-terminal-lost"));
  *host.attach_down.lock().unwrap() = false;
  h.click("remote-terminal-reattach");
  h.cx.run_until_parked();
  assert_eq!(host.attached().len(), 3);
  assert!(!h.drawn("remote-terminal-lost"));
}

#[gpui::test]
fn an_unreachable_remote_session_offers_reattach_and_close(cx: &mut TestAppContext) {
  let host = FakeHost {
    failing: vec!["remote_attach"],
    ..terminal_host()
  };
  let (mut h, host) = workspace_harness(cx, host, terminal_route(serde_json::json!({})));
  set_terminal(&mut h, open_remote_command(1));
  assert!(h.drawn("remote-terminal-lost"));
  assert_eq!(host.attached().len(), 2, "retried once first");
  h.click("remote-terminal-close");
  h.cx.run_until_parked();
  assert!(!h.drawn("terminal-column-shell"));
  assert!(host
    .calls()
    .iter()
    .any(|c| c.starts_with("close remote-pty-")));
}

#[gpui::test]
fn stopping_a_remote_session_ends_it_on_the_host(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, open_remote_command(1));
  h.click("remote-terminal-stop");
  h.cx.run_until_parked();
  assert!(host.calls().contains(&"stop shell-k1".to_string()));
  assert!(!h.drawn("terminal-column-shell"));
}

#[gpui::test]
fn detaching_a_remote_session_keeps_it_running(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, open_remote_command(1));
  h.click("remote-terminal-detach");
  h.cx.run_until_parked();
  assert!(!h.drawn("terminal-column-shell"));
  assert!(!host.calls().contains(&"stop shell-k1".to_string()));
  assert!(host
    .calls()
    .iter()
    .any(|c| c.starts_with("close remote-pty-")));
}

#[gpui::test]
fn a_failed_remote_write_shows_the_error_with_reattach(cx: &mut TestAppContext) {
  let host = FakeHost {
    failing: vec!["write_pty"],
    ..terminal_host()
  };
  let (mut h, host) = workspace_harness(cx, host, terminal_route(serde_json::json!({})));
  set_terminal(&mut h, open_remote_command(1));
  assert!(!h.drawn("remote-terminal-lost"));
  h.cx.simulate_input("ls");
  h.cx.run_until_parked();
  // The first failure reattaches on its own.
  assert_eq!(host.attached().len(), 2, "{:?}", host.calls());
  assert!(!h.drawn("remote-terminal-lost"));
  // Failing again before the session says anything shows the error.
  h.cx.simulate_input("ls");
  h.cx.run_until_parked();
  assert!(h.drawn("remote-terminal-lost"), "{:?}", host.calls());
  assert!(h.drawn("remote-terminal-reattach"));
  h.click("remote-terminal-reattach");
  h.cx.run_until_parked();
  assert_eq!(host.attached().len(), 3);
  assert!(!h.drawn("remote-terminal-lost"));
}

#[gpui::test]
fn a_remote_terminal_has_no_search(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, open_remote_command(1));
  h.cx.simulate_keystrokes("cmd-f");
  assert!(!h.drawn("terminal-search"));
  assert!(h.drawn("remote-terminal-header"));
}

fn drop_files(h: &mut Harness, at: gpui::Point<gpui::Pixels>) {
  let paths = gpui::ExternalPaths(["/tmp/a b.txt".into()].into_iter().collect());
  h.cx.simulate_event(gpui::FileDropEvent::Entered {
    position: at,
    paths,
  });
  h.cx
    .simulate_event(gpui::FileDropEvent::Submit { position: at });
  h.cx.run_until_parked();
}

#[gpui::test]
fn files_dropped_on_a_shell_are_typed_but_not_on_a_remote_session(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(
    &mut h,
    serde_json::json!({ "commands": [{ "nonce": 1, "kind": "newShell", "dir": "/repo" }] }),
  );
  let at = h
    .cx
    .debug_bounds("terminal-column-shell")
    .expect("shell")
    .center();
  drop_files(&mut h, at);
  let writes = |host: &FakeHost| {
    host
      .calls()
      .iter()
      .filter(|c| c.starts_with("write "))
      .count()
  };
  assert_eq!(writes(&host), 1, "{:?}", host.calls());

  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, open_remote_command(1));
  let at = h
    .cx
    .debug_bounds("terminal-column-shell")
    .expect("remote")
    .center();
  drop_files(&mut h, at);
  assert_eq!(writes(&host), 0, "{:?}", host.calls());
}

#[gpui::test]
fn double_clicking_a_remote_session_scrolls_it_fully_into_view(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  let open = |nonce: u64, label: &str| {
    let mut command = open_remote_command(nonce);
    command["commands"][0]["remote"]["label"] = label.into();
    command
  };
  set_terminal(&mut h, open(1, "shell-a"));
  let mut commands = open(1, "shell-a")["commands"].as_array().cloned().unwrap();
  for (nonce, label) in [(2, "shell-b"), (3, "shell-c")] {
    commands.extend(open(nonce, label)["commands"].as_array().cloned().unwrap());
  }
  set_terminal(&mut h, serde_json::json!({ "commands": commands }));
  let container = h
    .cx
    .debug_bounds("terminal-scroll-container")
    .expect("container");
  // The newest session opened in view; drag the row's scrollbar back to
  // the start so the last one overflows again.
  let thumb = h
    .cx
    .debug_bounds("terminal-row-scrollbar")
    .expect("scrollbar");
  let grab = thumb.center();
  let to = gpui::point(container.left(), grab.y);
  h.cx.simulate_event(gpui::MouseDownEvent {
    position: grab,
    modifiers: Modifiers::none(),
    button: MouseButton::Left,
    click_count: 1,
    first_mouse: false,
  });
  h.cx.simulate_event(gpui::MouseMoveEvent {
    position: to,
    modifiers: Modifiers::none(),
    pressed_button: Some(MouseButton::Left),
  });
  h.cx.simulate_event(gpui::MouseUpEvent {
    position: to,
    modifiers: Modifiers::none(),
    button: MouseButton::Left,
    click_count: 1,
  });
  h.cx.run_until_parked();
  let header = h.cx.debug_bounds("remote-terminal-header").expect("header");
  assert!(
    header.right() > container.right(),
    "the last session overflows the row: {header:?} {container:?}"
  );
  let at = gpui::point(container.right() - gpui::px(8.), header.center().y);
  for click_count in [1, 2] {
    h.cx.simulate_event(gpui::MouseDownEvent {
      position: at,
      modifiers: Modifiers::none(),
      button: MouseButton::Left,
      click_count,
      first_mouse: false,
    });
    h.cx.simulate_event(gpui::MouseUpEvent {
      position: at,
      modifiers: Modifiers::none(),
      button: MouseButton::Left,
      click_count,
    });
  }
  h.cx.run_until_parked();
  let header = h.cx.debug_bounds("remote-terminal-header").expect("header");
  assert!(
    header.right() <= container.right() + gpui::px(1.),
    "{header:?} {container:?}"
  );
}

#[gpui::test]
fn a_remote_repository_asks_react_for_new_shells(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(
    &mut h,
    serde_json::json!({
      "remoteShell": true,
      "commands": [{ "nonce": 1, "kind": "newShell", "dir": "/srv/app" }],
    }),
  );
  assert_eq!(
    fields(&h, "terminal.newRemoteShell"),
    vec![serde_json::json!("/srv/app")]
  );
  assert!(!host.calls().iter().any(|c| c.starts_with("spawn ")));
  assert!(!h.drawn("terminal-column-shell"));
}

#[gpui::test]
fn terminal_pane_tooltips_show_shortcuts_as_kbd(cx: &mut TestAppContext) {
  use crate::gpui_shell::popup::{PopupRequest, TooltipContent};
  let (mut h, _) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  let at = h
    .cx
    .debug_bounds("terminal-collapse")
    .expect("collapse")
    .center();
  h.cx.simulate_mouse_move(at, None, Modifiers::none());
  h.cx
    .executor()
    .advance_clock(std::time::Duration::from_millis(300));
  h.cx.run_until_parked();
  let shown = h.popups.borrow().iter().any(|p| {
    matches!(p, PopupRequest::ShowTooltip { content: TooltipContent::Shortcut { text, keys }, .. }
      if text == "Collapse" && *keys == crate::gpui_shell::keys::hint("⌘ + J"))
  });
  assert!(shown, "{:?}", h.popups.borrow());
}

#[gpui::test]
fn header_buttons_open_their_popovers_in_the_modal_window(cx: &mut TestAppContext) {
  for button in ["workspace-details", "workspace-target", "workspace-checks"] {
    let (mut h, _) = workspace_harness(cx, feature_host(), workspace_route());
    h.click(button);
    h.cx.run_until_parked();
    let opened = h
      .popups
      .borrow()
      .iter()
      .any(|p| matches!(p, PopupRequest::ShowPopover { .. }));
    assert!(opened, "{button}: {:?}", h.popups.borrow());
    // The screen stays: nothing is handed to React to draw.
    assert!(h.drawn("workspace-header"));
    assert!(h.sent().is_empty(), "{button}");
  }
}

#[gpui::test]
fn opening_details_hides_its_tooltip(cx: &mut TestAppContext) {
  use crate::gpui_shell::popup::PopupRequest;
  let (mut h, _) = workspace_harness(cx, feature_host(), workspace_route());
  let at = h
    .cx
    .debug_bounds("workspace-details")
    .expect("details")
    .center();
  h.cx.simulate_mouse_move(at, None, Modifiers::none());
  h.cx
    .executor()
    .advance_clock(std::time::Duration::from_millis(300));
  h.cx.run_until_parked();
  assert!(h
    .popups
    .borrow()
    .iter()
    .any(|p| matches!(p, PopupRequest::ShowTooltip { .. })));
  h.click("workspace-details");
  // Hovering again while the popover shows keeps the tooltip hidden.
  h.cx.simulate_mouse_move(
    at + gpui::point(gpui::px(1.), gpui::px(0.)),
    None,
    Modifiers::none(),
  );
  h.cx
    .executor()
    .advance_clock(std::time::Duration::from_millis(300));
  h.cx.run_until_parked();
  let popups = h.popups.borrow();
  let last_tooltip = popups.iter().rev().find(|p| {
    matches!(
      p,
      PopupRequest::ShowTooltip { .. } | PopupRequest::HideTooltip
    )
  });
  assert!(
    matches!(last_tooltip, Some(PopupRequest::HideTooltip)),
    "{popups:?}"
  );
  assert!(popups
    .iter()
    .any(|p| matches!(p, PopupRequest::ShowPopover { .. })));
}

fn send_assets() -> serde_json::Value {
  let pty = agent_pty();
  serde_json::json!([
    { "id": "t1", "repo": "/repo", "ptySessionId": pty, "mediaType": "text",
      "path": "/tmp/notes.txt", "title": "notes.txt" },
    { "id": "i1", "repo": "/repo", "ptySessionId": pty, "mediaType": "image",
      "path": "/tmp/treq-missing-shot.png", "title": "shot.png" },
    { "id": "b1", "repo": "/repo", "ptySessionId": pty, "mediaType": "browser",
      "path": "/tmp/page", "title": "page" },
    { "id": "o1", "repo": "/repo", "ptySessionId": "session-other-9", "mediaType": "text",
      "path": "/tmp/other.txt", "title": "other.txt" },
  ])
}

fn send_harness(cx: &mut TestAppContext) -> (Harness, Arc<FakeHost>) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  *host.send.lock().unwrap() = serde_json::from_value(send_assets()).expect("assets");
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  (h, host)
}

fn hover(h: &mut Harness, selector: &'static str) {
  h.cx.run_until_parked();
  let at = h.cx.debug_bounds(selector).expect(selector).center();
  h.cx.simulate_mouse_move(at, None, Modifiers::none());
  h.cx.run_until_parked();
}

fn pty_writes(host: &FakeHost) -> Vec<String> {
  host
    .calls()
    .into_iter()
    .filter(|c| c.starts_with("write "))
    .collect()
}

#[gpui::test]
fn terminal_shows_its_send_attachments_and_dismisses_them(cx: &mut TestAppContext) {
  let (mut h, host) = send_harness(cx);
  assert!(h.drawn("terminal-send-previews"));
  assert!(h.drawn("terminal-send-preview-t1"));
  assert!(h.drawn("terminal-send-preview-i1"));
  // Browser previews are the browser panel's; other terminals keep theirs.
  assert!(!h.drawn("terminal-send-preview-b1"));
  assert!(!h.drawn("terminal-send-preview-o1"));
  assert!(!h.drawn("terminal-send-dismiss-t1"));
  hover(&mut h, "terminal-send-preview-t1");
  assert!(h.drawn("terminal-send-dismiss-t1"));
  // Cmd+C over a thumbnail copies its quoted path.
  h.cx.simulate_keystrokes("cmd-c");
  assert_eq!(
    h.cx.read_from_clipboard().and_then(|c| c.text()).as_deref(),
    Some("'/tmp/notes.txt'")
  );
  hover(&mut h, "terminal-send-preview-t1");
  h.click("terminal-send-dismiss-t1");
  assert!(h.sent().contains(&ScreenAction::Field {
    field: "terminal.dismissAsset".into(),
    value: "t1".into(),
  }));
  assert!(host.calls().contains(&"dismiss asset t1".to_string()));
  assert!(!h.drawn("terminal-send-preview-t1"));
  assert!(!h.drawn("treq-send-preview-lightbox"));
}

#[gpui::test]
fn lightbox_sends_line_comments_on_a_text_attachment(cx: &mut TestAppContext) {
  let (mut h, host) = send_harness(cx);
  h.click("terminal-send-preview-t1");
  assert!(h.drawn("treq-send-preview-lightbox"));
  assert!(host
    .calls()
    .contains(&"read asset /tmp/notes.txt".to_string()));
  assert!(h.drawn("treq-send-text-line-3"));
  // Text has no zoom.
  assert!(!h.drawn("treq-send-zoom-in"));
  h.click("treq-send-text-line-2");
  assert!(h.drawn("treq-send-line-comment-composer"));
  h.cx.simulate_input("rename this");
  h.click("treq-send-line-comment-save");
  assert!(!h.drawn("treq-send-line-comment-composer"));
  assert!(h.drawn("treq-send-line-comment-1"));
  h.click("treq-send-review-input");
  h.cx.simulate_input("Thanks");
  h.cx.simulate_keystrokes("enter");
  h.cx.run_until_parked();
  let prompt = "Review of notes.txt\n\nFile: /tmp/notes.txt\n\nThanks\n\nInline comments:\n\nnotes.txt:2\n```\nsecond\n```\n> rename this\n";
  assert!(
    host.calls().contains(&format!("chat message 3 {prompt:?}")),
    "{:?}",
    host.calls()
  );
  let sent = format!("write {} {:?}", agent_pty(), format!("{prompt}\r"));
  assert!(pty_writes(&host).contains(&sent), "{:?}", pty_writes(&host));
  // Sent: nothing is left to lose on close.
  assert!(!h.drawn("treq-send-line-comment-1"));
  h.click("treq-send-close");
  assert!(!h.drawn("treq-send-preview-lightbox"));
}

#[gpui::test]
fn lightbox_asks_before_dropping_unsent_comments(cx: &mut TestAppContext) {
  let (mut h, _) = send_harness(cx);
  h.click("terminal-send-preview-t1");
  h.click("treq-send-close");
  assert!(!h.drawn("treq-send-preview-lightbox"));
  h.click("terminal-send-preview-t1");
  h.click("treq-send-review-input");
  h.cx.simulate_input("hmm");
  h.cx.simulate_keystrokes("escape");
  assert!(h.drawn("treq-send-unsaved-dialog"));
  h.click("treq-send-unsaved-keep");
  assert!(!h.drawn("treq-send-unsaved-dialog"));
  assert!(h.drawn("treq-send-preview-lightbox"));
  h.click("treq-send-close");
  h.click("treq-send-unsaved-discard");
  assert!(!h.drawn("treq-send-preview-lightbox"));
}

#[gpui::test]
fn lightbox_moves_between_attachments(cx: &mut TestAppContext) {
  let (mut h, _) = send_harness(cx);
  h.click("terminal-send-preview-t1");
  assert!(h.drawn("treq-send-text-preview"));
  h.click("treq-send-next");
  assert!(h.drawn("treq-send-highlight-surface"));
  assert!(h.drawn("treq-send-zoom-in"));
  h.cx.simulate_keystrokes("left");
  assert!(h.drawn("treq-send-text-preview"));
}

#[gpui::test]
fn lightbox_highlights_an_image_region_for_the_review(cx: &mut TestAppContext) {
  let (mut h, host) = send_harness(cx);
  h.click("terminal-send-preview-i1");
  let surface = h
    .cx
    .debug_bounds("treq-send-highlight-surface")
    .expect("surface");
  let width = f32::from(surface.size.width);
  // A click without a drag zooms in, and again out.
  h.click("treq-send-highlight-surface");
  assert!(h.drawn("treq-send-highlight-layer"));
  let zoomed = h.cx.debug_bounds("treq-send-highlight-layer").unwrap();
  assert!((f32::from(zoomed.size.width) - width * 2.).abs() < 1.);
  h.cx.simulate_click(
    zoomed.origin + gpui::point(gpui::px(50.), gpui::px(50.)),
    Modifiers::none(),
  );
  assert!(h.drawn("treq-send-highlight-surface"));
  let surface = h.cx.debug_bounds("treq-send-highlight-surface").unwrap();
  let from = surface.origin + gpui::point(gpui::px(10.), gpui::px(10.));
  let to = surface.origin + gpui::point(gpui::px(200.), gpui::px(150.));
  h.cx
    .simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
  h.cx
    .simulate_mouse_move(to, Some(MouseButton::Left), Modifiers::none());
  h.cx
    .simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
  assert!(h.drawn("treq-send-highlight-1"));
  assert!(h.drawn("treq-send-highlight-comment-1"));
  h.click("treq-send-review-input");
  h.cx.simulate_input("too big");
  h.cx.simulate_keystrokes("enter");
  h.cx.run_until_parked();
  // The image can't be read here, so the review goes without it.
  assert!(host
    .calls()
    .contains(&"notify toast Could not attach highlighted image".to_string()));
  let writes = pty_writes(&host);
  assert!(
    writes.iter().any(
      |w| w.contains("File: /tmp/treq-missing-shot.png") && w.contains("Highlights:\\n1. Region")
    ),
    "{writes:?}"
  );
  assert!(!h.drawn("treq-send-highlight-1"));
}

#[gpui::test]
fn terminal_reports_the_mouse_once_the_program_asks(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  let pty = agent_pty();
  h.click("terminal-body");
  assert!(pty_writes(&host).is_empty(), "no reports before asked");
  host.output(&pty, "\x1b[?1000h\x1b[?1006h");
  h.cx.run_until_parked();
  h.click("terminal-body");
  let writes = pty_writes(&host);
  assert_eq!(writes.len(), 2, "{writes:?}");
  assert!(
    writes[0].contains("[<0;") && writes[0].ends_with("M\""),
    "{writes:?}"
  );
  assert!(
    writes[1].contains("[<0;") && writes[1].ends_with("m\""),
    "{writes:?}"
  );
}

// The dialogs drawn over a screen: agent prompt, prompt history and stashes.

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

fn history_modal() -> serde_json::Value {
  serde_json::json!({ "kind": "promptHistory", "repoPath": "/repo" })
}

fn history_host() -> FakeHost {
  FakeHost {
    prompts: vec![
      prompt_entry(
        3,
        Some(7),
        Some("login fixes"),
        "Fix the login button",
        Some("claude"),
      ),
      prompt_entry(2, None, None, "Refactor the\nrouter", None),
      prompt_entry(1, Some(9), None, "Gone", Some("codex")),
    ],
    ..FakeHost::default()
  }
}

#[test]
fn prompt_history_names_workspaces_as_react_does() {
  use modal::prompt_history::{format_for_copy, workspace_label};
  let entries = [
    prompt_entry(1, Some(7), Some("login"), "a", None),
    prompt_entry(2, None, None, "b", None),
    prompt_entry(3, Some(9), None, "c", None),
  ];
  assert_eq!(workspace_label(&entries[0]), "login");
  assert_eq!(workspace_label(&entries[1]), "Home repo");
  assert_eq!(workspace_label(&entries[2]), "Deleted workspace");
  let copied = format_for_copy(&entries[..2]);
  assert!(copied.starts_with("[login] 10/19/20"), "{copied}");
  assert!(copied.contains("\na\n\n---\n\n[Home repo] "), "{copied}");
  assert!(copied.ends_with("\nb"), "{copied}");
}

#[gpui::test]
fn prompt_history_shows_the_newest_prompt_first(cx: &mut TestAppContext) {
  let (mut h, host) = modal_harness(cx, history_host(), history_modal());
  for selector in [
    "prompt-history-modal",
    "prompt-history-list",
    "prompt-history-item-0",
    "prompt-history-item-2",
    "prompt-history-detail-text",
    "prompt-history-copy-all",
    "prompt-history-run",
  ] {
    assert!(h.drawn(selector), "{selector}");
  }
  assert!(host.calls().contains(&"read prompt history".to_string()));
  let selected = h.screen.read_with(&h.cx, |s, cx| {
    s.prompt_history.as_ref().map(|v| v.read(cx).selected)
  });
  assert_eq!(selected, Some(Some(3)));
  h.click("prompt-history-item-1");
  let selected = h.screen.read_with(&h.cx, |s, cx| {
    s.prompt_history.as_ref().map(|v| v.read(cx).selected)
  });
  assert_eq!(selected, Some(Some(2)));
}

#[gpui::test]
fn prompt_history_opens_on_the_requested_prompt(cx: &mut TestAppContext) {
  let (h, _) = modal_harness(
    cx,
    history_host(),
    serde_json::json!({ "kind": "promptHistory", "repoPath": "/repo", "initialSelectedId": 1 }),
  );
  let selected = h.screen.read_with(&h.cx, |s, cx| {
    s.prompt_history.as_ref().map(|v| v.read(cx).selected)
  });
  assert_eq!(selected, Some(Some(1)));
}

#[gpui::test]
fn prompt_history_copies_and_runs_prompts(cx: &mut TestAppContext) {
  let (mut h, host) = modal_harness(cx, history_host(), history_modal());
  h.click("prompt-history-copy");
  assert_eq!(h.clipboard().as_deref(), Some("Fix the login button"));
  h.click("prompt-history-copy-all");
  let all = h.clipboard().expect("clipboard");
  assert!(all.starts_with("[login fixes] 10/19/2026, "), "{all}");
  assert!(all.contains("\n\n---\n\n[Home repo] "), "{all}");
  assert_eq!(
    host.writes(),
    [
      "notify toast Prompt copied",
      "notify toast All prompts copied"
    ]
  );
  h.click("prompt-history-item-2");
  h.click("prompt-history-run");
  assert_eq!(
    fields(&h, "promptHistory.run"),
    vec![serde_json::json!({ "prompt": "Gone", "workspaceId": 9 })]
  );
}

#[gpui::test]
fn prompt_history_closes_through_react(cx: &mut TestAppContext) {
  let (mut h, _) = modal_harness(cx, history_host(), history_modal());
  h.click_backdrop();
  h.cx.simulate_keystrokes("escape");
  h.cx.run_until_parked();
  assert_eq!(
    h.sent(),
    vec![press("promptHistory.close"), press("promptHistory.close")]
  );
}

#[gpui::test]
fn prompt_history_says_when_there_is_nothing(cx: &mut TestAppContext) {
  let (mut h, _) = modal_harness(cx, FakeHost::default(), history_modal());
  assert!(h.drawn("prompt-history-empty"));
  assert!(!h.drawn("prompt-history-copy-all"));
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

#[gpui::test]
fn stash_modal_lists_stashes_with_the_first_diff(cx: &mut TestAppContext) {
  let (mut h, host) = modal_harness(cx, stash_host(), stash_modal());
  for selector in [
    "stash-modal",
    "stash-list-item-0",
    "stash-list-item-1",
    "stash-detail",
    "stash-diff-pane",
    "stash-diff-file-0",
    "stash-apply-button",
    "stash-more-menu",
  ] {
    assert!(h.drawn(selector), "{selector}");
  }
  assert!(host.calls().contains(&"read stashes".to_string()));
  assert!(host.calls().contains(&"read stash diff 1".to_string()));
  h.click("stash-list-item-1");
  assert!(host.calls().contains(&"read stash diff 2".to_string()));
}

#[gpui::test]
fn stash_modal_applies_a_stash_to_a_workspace(cx: &mut TestAppContext) {
  let (mut h, host) = modal_harness(cx, stash_host(), stash_modal());
  h.click("stash-apply-button");
  for selector in [
    "stash-apply-popover",
    "stash-apply-workspace-select",
    "stash-apply-option-.",
    "stash-apply-option-treq/login",
    "stash-apply-option-treq/docs",
    "stash-apply-new-workspace",
  ] {
    assert!(h.drawn(selector), "{selector}");
  }
  h.click("stash-apply-option-treq/login");
  h.cx.run_until_parked();
  assert!(!h.drawn("stash-apply-popover"));
  assert_eq!(
    host.writes(),
    [
      "apply stash 1 treq/login",
      "notify workspaceChanged",
      "notify toast Stash applied"
    ]
  );
}

#[gpui::test]
fn stash_modal_applies_to_the_home_repo(cx: &mut TestAppContext) {
  let (mut h, host) = modal_harness(cx, stash_host(), stash_modal());
  h.click("stash-apply-button");
  h.click("stash-apply-option-.");
  h.cx.run_until_parked();
  assert!(host.writes().contains(&"apply stash 1 .".to_string()));
}

#[gpui::test]
fn stash_modal_reports_a_failed_apply(cx: &mut TestAppContext) {
  let host = FakeHost {
    apply_error: Some("conflict".into()),
    ..stash_host()
  };
  let (mut h, host) = modal_harness(cx, host, stash_modal());
  h.click("stash-apply-button");
  h.click("stash-apply-option-treq/docs");
  h.cx.run_until_parked();
  assert_eq!(
    host.writes(),
    [
      "apply stash 1 treq/docs",
      "notify toast Failed to apply stash"
    ]
  );
}

#[gpui::test]
fn stash_modal_applies_to_a_new_workspace_in_react(cx: &mut TestAppContext) {
  let (mut h, _) = modal_harness(cx, stash_host(), stash_modal());
  h.click("stash-apply-button");
  h.click("stash-apply-new-workspace");
  h.cx.run_until_parked();
  let sent = fields(&h, "stash.applyToNew");
  assert_eq!(sent.len(), 1);
  assert_eq!(sent[0]["id"], 1);
  assert_eq!(sent[0]["short_commit_id"], "abc1");
  assert!(!h.drawn("stash-apply-popover"));
}

#[gpui::test]
fn stash_modal_copies_a_patch_and_deletes_a_stash(cx: &mut TestAppContext) {
  let (mut h, host) = modal_harness(cx, stash_host(), stash_modal());
  h.click("stash-more-menu");
  h.click("stash-copy-patch");
  h.cx.run_until_parked();
  assert_eq!(h.clipboard().as_deref(), Some("patch of stash 1"));
  h.click("stash-more-menu");
  h.click("stash-delete");
  h.cx.run_until_parked();
  assert!(host.writes().contains(&"delete stash 1".to_string()));
  assert!(host
    .writes()
    .contains(&"notify toast Stash deleted".to_string()));
  // The list reloads and the next stash is selected.
  assert!(h.drawn("stash-list-item-0"));
  assert!(!h.drawn("stash-list-item-1"));
  let selected = h
    .screen
    .read_with(&h.cx, |s, cx| s.stash.as_ref().map(|v| v.read(cx).selected));
  assert_eq!(selected, Some(Some(2)));
}

#[gpui::test]
fn stash_modal_explains_an_empty_list(cx: &mut TestAppContext) {
  let (mut h, _) = modal_harness(cx, FakeHost::default(), stash_modal());
  assert!(h.drawn("stash-empty"));
  assert!(!h.drawn("stash-list"));
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

fn agent_view<T>(h: &Harness, f: impl FnOnce(&modal::agent_prompt::AgentPromptView) -> T) -> T {
  h.screen.read_with(&h.cx, |s, cx| {
    f(s.agent_prompt.as_ref().expect("agent prompt").read(cx))
  })
}

use code_data::Agent;

#[gpui::test]
fn agent_prompt_starts_with_what_react_sent(cx: &mut TestAppContext) {
  let (mut h, _) = modal_harness(cx, FakeHost::default(), agent_modal());
  for selector in [
    "agent-prompt-dialog",
    "agent-prompt-workspace",
    "agent-task-input",
    "task-input",
    "task-add",
    "task-agent",
    "task-submit",
    "task-submit-menu",
  ] {
    assert!(h.drawn(selector), "{selector}");
  }
  assert_eq!(agent_view(&h, |v| v.selected), Some(7));
  assert_eq!(agent_view(&h, |v| v.task_text.clone()), "fix it");
  assert_eq!(
    agent_view(&h, |v| v.agents()),
    (Agent::Claude, Agent::Claude)
  );
}

/// The prompt dialog's starts the backend was asked for.
fn tasks(h: &mut Harness, host: &FakeHost) -> Vec<launch_data::TaskRequest> {
  h.cx.run_until_parked();
  host.tasks.lock().unwrap().clone()
}

#[gpui::test]
fn agent_prompt_starts_the_session_in_the_backend(cx: &mut TestAppContext) {
  let (mut h, host) = modal_harness(cx, FakeHost::default(), agent_modal());
  h.click("task-submit");
  // Until the dialog closes, the prompt is not sent twice.
  h.click("task-submit");
  assert_eq!(
    tasks(&mut h, &host),
    vec![launch_data::TaskRequest {
      repo_path: "/repo".into(),
      workspace_id: Some(7),
      text: "fix it".into(),
      issue: None,
      agent: "claude".into(),
      mode: "acceptEdits".into(),
      save_as_repo_default: false,
    }]
  );
  // Started: React closes the dialog; nothing else goes to React.
  assert_eq!(h.sent(), vec![press("agentPrompt.close")]);
  assert!(agent_view(&h, |v| v.submitting()));
}

#[gpui::test]
fn agent_prompt_toasts_a_failed_start_and_allows_another(cx: &mut TestAppContext) {
  let host = FakeHost {
    task_error: Some("jj failed".into()),
    ..FakeHost::default()
  };
  let (mut h, host) = modal_harness(cx, host, agent_modal());
  h.click("task-submit");
  assert_eq!(tasks(&mut h, &host).len(), 1);
  assert!(host
    .calls()
    .contains(&"notify toast Failed to create task".to_string()));
  assert!(h.sent().is_empty());
  assert!(!agent_view(&h, |v| v.submitting()));
}

#[gpui::test]
fn agent_prompt_plans_instead_of_editing(cx: &mut TestAppContext) {
  let (mut h, host) = modal_harness(cx, FakeHost::default(), agent_modal());
  h.click("task-submit-menu");
  h.click("task-plan");
  assert_eq!(tasks(&mut h, &host)[0].mode, "plan");
}

#[gpui::test]
fn agent_prompt_runs_the_default_agent_unless_changed(cx: &mut TestAppContext) {
  let host = FakeHost::with_settings(&[("/repo:default_agent", "codex")]);
  let (mut h, host) = modal_harness(cx, host, agent_modal());
  assert_eq!(agent_view(&h, |v| v.agents()), (Agent::Codex, Agent::Codex));
  // Codex has no plan mode.
  assert!(!h.drawn("task-submit-menu"));
  h.click("task-agent");
  assert!(h.drawn("task-agent-menu"));
  h.click("task-agent-cursor");
  assert_eq!(
    agent_view(&h, |v| v.agents()),
    (Agent::Codex, Agent::Cursor)
  );
  assert!(h.drawn("task-save-default-box"));
  assert!(h.drawn("task-submit-menu"));
  h.click("task-save-default-box");
  h.click("task-submit");
  let start = &tasks(&mut h, &host)[0];
  assert_eq!(start.agent, "cursor");
  assert!(start.save_as_repo_default);
}

#[gpui::test]
fn agent_prompt_picks_a_workspace(cx: &mut TestAppContext) {
  let (mut h, host) = modal_harness(cx, FakeHost::default(), agent_modal());
  h.click("agent-prompt-workspace");
  assert!(h.drawn("agent-prompt-workspace-popover"));
  assert!(h.drawn("agent-prompt-workspace-main"));
  assert!(h.drawn("agent-prompt-workspace-treq/login"));
  h.click("agent-prompt-workspace-main");
  assert_eq!(agent_view(&h, |v| v.selected), None);
  assert!(!h.drawn("agent-prompt-workspace-popover"));
  h.click("task-submit");
  assert_eq!(tasks(&mut h, &host)[0].workspace_id, None);
}

#[gpui::test]
fn agent_prompt_runs_an_attached_issue_in_its_own_workspace(cx: &mut TestAppContext) {
  let mut modal = agent_modal();
  modal["initialPrompt"] = serde_json::Value::Null;
  modal["initialWorkspaceId"] = serde_json::Value::Null;
  modal["issue"] = serde_json::json!({
    "source": "linear",
    "key": "ENG-101",
    "title": "Login is slow",
    "includeSubItems": true,
    "id": "issue-1",
    "url": "https://linear.app/t/ENG-101",
    "subItemIds": ["child-1"],
  });
  let (mut h, host) = modal_harness(cx, FakeHost::default(), modal);
  assert!(h.drawn("agent-prompt-issue-note"));
  assert!(h.drawn("issue-chip"));
  assert!(!h.drawn("agent-prompt-workspace"));
  // An issue alone is enough to start.
  h.click("task-submit");
  let start = &tasks(&mut h, &host)[0];
  assert_eq!(start.text, "");
  assert_eq!(
    start.issue,
    Some(crate::core::agent_launch::IssueAttachment {
      source: crate::core::agent_launch::IssueSource::Linear,
      id: "issue-1".into(),
      key: "ENG-101".into(),
      url: "https://linear.app/t/ENG-101".into(),
      title: "Login is slow".into(),
      include_sub_items: true,
      sub_item_ids: vec!["child-1".into()],
    })
  );
}

#[gpui::test]
fn agent_prompt_detaches_an_issue(cx: &mut TestAppContext) {
  let mut modal = agent_modal();
  modal["issue"] = serde_json::json!({
    "source": "github",
    "key": "#42",
    "title": "Bug",
    "includeSubItems": false,
  });
  let (mut h, host) = modal_harness(cx, FakeHost::default(), modal);
  h.click("issue-chip-remove");
  assert!(!h.drawn("issue-chip"));
  assert!(h.drawn("agent-prompt-workspace"));
  h.click("task-submit");
  assert_eq!(tasks(&mut h, &host)[0].issue, None);
}

#[gpui::test]
fn agent_prompt_suggests_files_for_mentions(cx: &mut TestAppContext) {
  let host = FakeHost {
    files: vec!["src/login.rs".into(), "src/logout.rs".into()],
    ..FakeHost::default()
  };
  let mut modal = agent_modal();
  modal["initialPrompt"] = serde_json::Value::Null;
  let (mut h, _) = modal_harness(cx, host, modal);
  h.click("agent-task-input");
  h.cx.simulate_input("look at @log");
  h.cx
    .executor()
    .advance_clock(std::time::Duration::from_millis(500));
  assert!(h.drawn("task-mention-0"));
  h.click("task-mention-1");
  assert_eq!(
    agent_view(&h, |v| v.task_text.clone()),
    "look at @src/logout.rs "
  );
}

#[gpui::test]
fn agent_prompt_waits_while_react_starts_the_session(cx: &mut TestAppContext) {
  let mut modal = agent_modal();
  modal["starting"] = true.into();
  let (mut h, host) = modal_harness(cx, FakeHost::default(), modal);
  h.click("task-submit");
  assert!(tasks(&mut h, &host).is_empty());
  assert!(agent_view(&h, |v| v.submitting()));
}

#[gpui::test]
fn agent_prompt_keeps_its_text_while_react_refreshes(cx: &mut TestAppContext) {
  let (mut h, _) = modal_harness(cx, FakeHost::default(), agent_modal());
  h.click("agent-task-input");
  h.cx.simulate_input(" now");
  let mut refreshed = agent_modal();
  refreshed["workspaces"] = serde_json::json!([]);
  h.set_modal(Some(refreshed));
  // The workspace went away; the text stays.
  assert_eq!(agent_view(&h, |v| v.selected), None);
  assert!(agent_view(&h, |v| v.task_text.contains("now")));
}

#[gpui::test]
fn agent_prompt_closes_through_react(cx: &mut TestAppContext) {
  let (mut h, _) = modal_harness(cx, FakeHost::default(), agent_modal());
  h.click_backdrop();
  assert_eq!(h.sent(), vec![press("agentPrompt.close")]);
}

#[gpui::test]
fn dialogs_cover_the_screen_until_closed(cx: &mut TestAppContext) {
  let (mut h, _) = modal_harness(cx, FakeHost::default(), history_modal());
  assert!(h.drawn("prompt-history-modal"));
  assert!(h.drawn("onboarding-open-repo"));
  h.set_modal(None);
  assert!(!h.drawn("prompt-history-modal"));
  // Another kind replaces the first.
  h.set_modal(Some(stash_modal()));
  assert!(h.drawn("stash-modal"));
}

// -- The remote dialogs ------------------------------------------------------

fn remote_snapshot(remote: serde_json::Value) -> ScreenSnapshot {
  serde_json::from_value(serde_json::json!({
    "kind": "onboarding",
    "dark": false,
    "remote": remote,
  }))
  .expect("parse")
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

#[gpui::test]
fn setup_dialog_opens_on_your_own_vm_from_the_ssh_hosts_plus(cx: &mut TestAppContext) {
  let (mut h, _) = remote_harness(
    cx,
    FakeHost::default(),
    serde_json::json!({ "setup": { "allowUserManaged": true, "screen": "ownVm" } }),
  );
  assert!(h.drawn("remote-username"));
  assert!(!h.drawn("remote-setup-own-vm"));
  // An action pair and no X; Cancel closes, as there is no choice to go
  // back to.
  assert!(!h.drawn("remote-setup-close"));
  assert!(!h.drawn("remote-setup-back"));
  h.click("remote-setup-cancel");
  assert_eq!(h.sent(), vec![press("remote.setup.close")]);
}

#[gpui::test]
fn setup_dialog_asks_to_sign_in_before_the_cloud_workspace(cx: &mut TestAppContext) {
  let (mut h, host) = remote_harness(
    cx,
    FakeHost::default(),
    serde_json::json!({ "setup": { "allowUserManaged": true, "screen": "managed" } }),
  );
  // The machines page's cloud row opens straight on the cloud workspace.
  assert!(h.drawn("cloud-create"));
  assert!(!h.drawn("remote-setup-own-vm"));
  h.set_account(account_state(serde_json::json!({})));
  assert!(h.drawn("remote-setup-signed-out"));
  assert!(!h.drawn("cloud-create"));
  h.click("remote-setup-sign-in");
  h.cx.run_until_parked();
  assert!(host
    .writes()
    .contains(&"open http://localhost:3001/sign-in?source=desktop".to_string()));
  h.set_account(account_state(serde_json::json!({ "user": ada() })));
  assert!(h.drawn("cloud-create"));
}

fn ssh_key(label: &str) -> crate::core::remote_local_keys::LocalSshIdentity {
  crate::core::remote_local_keys::LocalSshIdentity {
    reference: format!("/home/me/.ssh/{label}.pub"),
    label: label.into(),
    fingerprint_sha256: "SHA256:key".into(),
    algorithm: "ssh-ed25519".into(),
  }
}

fn endpoint_json() -> serde_json::Value {
  serde_json::json!({
    "id": "user-managed-1",
    "instance_id": null,
    "source": { "type": "user_managed" },
    "hostname": "devbox.example",
    "port": 22,
    "username": "me",
    "host_keys": [],
    "authentication": { "type": "public_key", "key_reference": "/home/me/.ssh/id_ed25519.pub" },
  })
}

fn terminal_remote_route() -> serde_json::Value {
  serde_json::json!({
    "terminal": {
      "endpoint": endpoint_json(),
      "repositoryId": "/srv/project",
      "workspaceId": "root",
    }
  })
}

fn remote_session(
  label: &str,
  running: bool,
) -> crate::core::pty_remote_supervisor::PtySessionInfo {
  crate::core::pty_remote_supervisor::PtySessionInfo {
    session_name: format!("treq-pty-{label}"),
    workspace: "root".into(),
    label: label.into(),
    running,
  }
}

#[test]
fn parses_the_remote_dialogs_from_the_snapshot() {
  let snapshot = remote_snapshot(serde_json::json!({
    "setup": { "allowUserManaged": true, "registering": true },
    "ambiguous": "connection reset",
  }));
  let setup = snapshot.remote.setup.as_ref().expect("setup");
  assert!(setup.allow_user_managed && setup.registering);
  assert!(snapshot.remote.is_open());
  assert!(!remote_snapshot(serde_json::json!({})).remote.is_open());
  // An empty reason is no reason, as `Boolean(reason)` reads it.
  assert!(!remote_snapshot(serde_json::json!({ "ambiguous": "" }))
    .remote
    .is_open());
  let terminal = remote_snapshot(terminal_remote_route());
  assert_eq!(
    terminal
      .remote
      .terminal
      .expect("terminal")
      .endpoint
      .hostname,
    "devbox.example"
  );
}

#[gpui::test]
fn setup_dialog_offers_both_paths_and_goes_back(cx: &mut TestAppContext) {
  let (mut h, _) = remote_harness(
    cx,
    FakeHost::default(),
    setup_route(true, serde_json::json!({})),
  );
  assert!(h.drawn("remote-setup-dialog"));
  assert!(h.drawn("remote-setup-managed"));
  assert!(h.drawn("remote-setup-own-vm"));
  assert!(!h.drawn("remote-setup-back"));
  h.click("remote-setup-managed");
  assert!(h.drawn("cloud-create"));
  assert!(!h.drawn("remote-setup-managed"));
  h.click("remote-setup-back");
  assert!(h.drawn("remote-setup-managed"));
  assert!(!h.drawn("cloud-create"));
}

#[gpui::test]
fn setup_dialog_opens_on_the_cloud_workspace_without_the_preview(cx: &mut TestAppContext) {
  let (mut h, _) = remote_harness(
    cx,
    FakeHost::default(),
    setup_route(false, serde_json::json!({})),
  );
  assert!(h.drawn("cloud-create"));
  assert!(!h.drawn("remote-setup-managed"));
  // Nothing to go back to.
  assert!(!h.drawn("remote-setup-back"));
}

#[gpui::test]
fn setup_dialog_runs_the_cloud_workspace_flows_in_rust(cx: &mut TestAppContext) {
  let (mut h, host) = remote_harness(
    cx,
    FakeHost::default(),
    setup_route(false, serde_json::json!({})),
  );
  // Opening it reads the cloud workspace afresh.
  assert!(host.calls().contains(&"read cloud workspace".to_string()));
  h.click("cloud-create");

  let failed = serde_json::json!({
    "instance": { "status": "failed", "providerResourceId": "sprite-1", "diskQuotaGb": 10 },
    "endpoint": { "username": "treq", "hostname": "vm.example", "port": 22 },
  });
  h.set_account(account_state(
    serde_json::json!({ "user": ada(), "cloud": failed }),
  ));
  h.click("cloud-repair");
  // Deleting asks first, in a confirmation of its own.
  h.click("cloud-delete");
  assert!(h.drawn("cloud-delete-dialog"));
  h.click("cloud-delete-cancel");
  assert!(!h.drawn("cloud-delete-dialog"));
  h.click("cloud-delete");
  h.click("cloud-delete-confirm");
  assert!(!h.drawn("cloud-delete-dialog"));

  let ready = serde_json::json!({
    "instance": { "status": "ready", "providerResourceId": "sprite-1", "diskQuotaGb": 10 },
    "endpoint": { "username": "treq", "hostname": "vm.example", "port": 22 },
  });
  h.set_account(account_state(
    serde_json::json!({ "user": ada(), "cloud": ready }),
  ));
  h.click("cloud-open-repositories");
  assert!(h.sent().is_empty());

  let pro = serde_json::json!({ "provisioningError": "[pro_required] Upgrade to create one." });
  h.set_account(account_state(
    serde_json::json!({ "user": ada(), "cloud": pro }),
  ));
  h.click("cloud-upgrade");
  h.cx.run_until_parked();
  let writes = host.writes();
  assert_eq!(
    writes[..4],
    [
      "cloud Provision",
      "cloud Repair",
      "cloud Delete",
      "cloud OpenRepositories"
    ]
  );
  assert_eq!(writes.len(), 5);
  assert!(writes[4].starts_with("open ") && writes[4].ends_with("/dashboard"));
}

#[gpui::test]
fn setup_dialog_closes_from_its_backdrop_only(cx: &mut TestAppContext) {
  let (mut h, _) = remote_harness(
    cx,
    FakeHost::default(),
    setup_route(true, serde_json::json!({})),
  );
  assert!(h.drawn("remote-setup-dialog"));
  // A click in the panel stays in the dialog.
  h.click("remote-setup-dialog");
  assert!(h.sent().is_empty());
  h.cx
    .simulate_click(gpui::point(gpui::px(5.), gpui::px(5.)), Modifiers::none());
  assert_eq!(h.sent(), vec![press("remote.setup.close")]);
}

fn own_vm_host() -> FakeHost {
  FakeHost {
    ssh_keys: vec![ssh_key("id_rsa"), ssh_key("id_ed25519")],
    ..FakeHost::default()
  }
}

fn type_into(h: &mut Harness, selector: &'static str, text: &str) {
  h.click(selector);
  h.cx.simulate_input(text);
}

#[gpui::test]
fn user_managed_form_confirms_the_host_key_before_registering(cx: &mut TestAppContext) {
  let (mut h, host) = remote_harness(cx, own_vm_host(), setup_route(true, serde_json::json!({})));
  h.click("remote-setup-own-vm");
  assert!(h.drawn("remote-hostname"));
  // Continue waits for a hostname and a username; the key is the default.
  h.click("remote-continue");
  assert!(host.writes().is_empty());
  type_into(&mut h, "remote-hostname", " devbox.example ");
  h.click("remote-continue");
  assert!(host.writes().is_empty());
  type_into(&mut h, "remote-username", "me");
  h.click("remote-continue");
  h.cx.run_until_parked();
  assert_eq!(host.writes(), ["probe devbox.example:22"]);
  assert!(h.drawn("remote-trust-confirmation"));
  // Nothing is registered until trust is confirmed, and Continue is gone.
  assert!(h.sent().is_empty());
  assert!(!h.drawn("remote-continue"));

  h.click("remote-trust-connect");
  assert_eq!(
    fields(&h, "remote.registerUserManaged"),
    vec![serde_json::json!({
      "display_name": "devbox.example",
      "hostname": "devbox.example",
      "port": 22,
      "username": "me",
      "host_key_fingerprint": "SHA256:abc123",
      "auth_identity_reference": "/home/me/.ssh/id_ed25519.pub",
      "alias": null,
    })]
  );
}

#[gpui::test]
fn user_managed_form_can_decline_the_host_key(cx: &mut TestAppContext) {
  let (mut h, host) = remote_harness(cx, own_vm_host(), setup_route(true, serde_json::json!({})));
  h.click("remote-setup-own-vm");
  type_into(&mut h, "remote-display-name", "Dev box");
  type_into(&mut h, "remote-hostname", "10.0.0.4");
  type_into(&mut h, "remote-username", "me");
  // Typed after the default 22.
  type_into(&mut h, "remote-port", "22");
  h.click("remote-continue");
  h.cx.run_until_parked();
  assert_eq!(host.writes(), ["probe 10.0.0.4:2222"]);
  h.click("remote-trust-cancel");
  assert!(!h.drawn("remote-trust-confirmation"));
  assert!(h.drawn("remote-continue"));
  assert!(h.sent().is_empty());
  h.click("remote-continue");
  h.click("remote-trust-connect");
  let values = fields(&h, "remote.registerUserManaged");
  assert_eq!(values[0]["display_name"], "Dev box");
  assert_eq!(values[0]["port"], 2222);
}

#[gpui::test]
fn user_managed_form_shows_why_the_host_key_failed(cx: &mut TestAppContext) {
  let host = FakeHost {
    host_key: Some(Err("connection refused".into())),
    ..own_vm_host()
  };
  let (mut h, _) = remote_harness(cx, host, setup_route(true, serde_json::json!({})));
  h.click("remote-setup-own-vm");
  type_into(&mut h, "remote-hostname", "10.0.0.4");
  type_into(&mut h, "remote-username", "me");
  h.click("remote-continue");
  assert!(h.drawn("remote-probe-error"));
  assert!(!h.drawn("remote-trust-confirmation"));
  assert!(h.drawn("remote-continue"));
}

#[gpui::test]
fn user_managed_form_fills_from_an_ssh_alias(cx: &mut TestAppContext) {
  let host = FakeHost {
    resolved_alias: Some(Ok(crate::core::remote_ssh_config::ResolvedSshAlias {
      alias: "prod".into(),
      hostname: "prod.example".into(),
      port: 2200,
      username: Some("deploy".into()),
      identity_file: None,
    })),
    ..own_vm_host()
  };
  let (mut h, host) = remote_harness(cx, host, setup_route(true, serde_json::json!({})));
  h.click("remote-setup-own-vm");
  assert!(!h.drawn("remote-alias"));
  h.click("remote-alias-toggle");
  assert!(h.drawn("remote-alias"));
  h.click("remote-alias-autofill");
  // Needs an alias first.
  assert!(host.writes().is_empty());
  type_into(&mut h, "remote-alias", "prod");
  h.click("remote-alias-autofill");
  h.cx.run_until_parked();
  assert_eq!(host.writes(), ["resolve prod"]);
  h.click("remote-continue");
  h.cx.run_until_parked();
  // The alias only filled the fields: the host key is still confirmed.
  assert_eq!(host.writes(), ["resolve prod", "probe prod.example:2200"]);
  h.click("remote-trust-connect");
  let values = fields(&h, "remote.registerUserManaged");
  assert_eq!(values[0]["username"], "deploy");
  assert_eq!(values[0]["alias"], "prod");
}

#[gpui::test]
fn user_managed_form_is_locked_while_react_registers(cx: &mut TestAppContext) {
  let (mut h, _) = remote_harness(cx, own_vm_host(), setup_route(true, serde_json::json!({})));
  h.click("remote-setup-own-vm");
  type_into(&mut h, "remote-hostname", "10.0.0.4");
  type_into(&mut h, "remote-username", "me");
  h.click("remote-continue");
  h.click("remote-trust-connect");
  // Back is disabled until React is done.
  h.click("remote-setup-back");
  assert!(h.drawn("remote-hostname"));
  for registering in [true, false] {
    h.update(|s| s.remote.setup.as_mut().expect("setup").registering = registering);
  }
  h.click("remote-setup-back");
  assert!(h.drawn("remote-setup-managed"));
}

#[gpui::test]
fn terminal_dialog_lists_sessions_to_reattach(cx: &mut TestAppContext) {
  let host = FakeHost {
    remote_sessions: Some(Ok(vec![
      remote_session("claude-1", true),
      remote_session("shell-2", false),
    ])),
    ..FakeHost::default()
  };
  let (mut h, host) = remote_harness(cx, host, terminal_remote_route());
  assert!(h.drawn("remote-terminal-dialog"));
  assert_eq!(host.writes(), ["sessions devbox.example:/srv/project:root"]);
  assert!(h.drawn("remote-reattach-1"));
  // A stopped session cannot be reattached.
  h.click("remote-reattach-1");
  assert!(h.sent().is_empty());
  h.click("remote-reattach-0");
  h.click("remote-start-new");
  assert_eq!(
    fields(&h, "remote.terminal.open"),
    vec![
      serde_json::json!({ "label": "claude-1", "reattach": true }),
      serde_json::json!({ "label": null, "reattach": false }),
    ]
  );
}

#[gpui::test]
fn terminal_dialog_starts_a_session_when_listing_fails(cx: &mut TestAppContext) {
  let host = FakeHost {
    remote_sessions: Some(Err("ssh: connect refused".into())),
    ..FakeHost::default()
  };
  let (mut h, _) = remote_harness(cx, host, terminal_remote_route());
  assert!(h.drawn("remote-terminal-error"));
  assert!(!h.drawn("remote-reattach-0"));
  h.click("remote-start-new");
  assert_eq!(
    fields(&h, "remote.terminal.open"),
    vec![serde_json::json!({ "label": null, "reattach": false })]
  );
}

#[gpui::test]
fn terminal_dialog_lists_again_for_another_workspace(cx: &mut TestAppContext) {
  let (mut h, host) = remote_harness(cx, FakeHost::default(), terminal_remote_route());
  assert!(h.drawn("remote-start-new"));
  h.update(|s| s.remote.terminal.as_mut().expect("terminal").workspace_id = "feature".into());
  h.cx.run_until_parked();
  assert_eq!(
    host.writes(),
    [
      "sessions devbox.example:/srv/project:root",
      "sessions devbox.example:/srv/project:feature"
    ]
  );
  h.click("remote-terminal-dialog");
  assert!(h.sent().is_empty());
  h.cx
    .simulate_click(gpui::point(gpui::px(5.), gpui::px(5.)), Modifiers::none());
  assert_eq!(h.sent(), vec![press("remote.terminal.close")]);
}

#[gpui::test]
fn ambiguous_dialog_dismisses_or_refreshes(cx: &mut TestAppContext) {
  let (mut h, _) = remote_harness(
    cx,
    FakeHost::default(),
    serde_json::json!({ "ambiguous": "Connection reset while pushing" }),
  );
  assert!(h.drawn("remote-ambiguous-dialog"));
  assert!(h.drawn("remote-ambiguous-reason"));
  h.click("remote-ambiguous-dismiss");
  h.click("remote-ambiguous-refresh");
  assert_eq!(
    h.sent(),
    vec![
      press("remote.ambiguous.dismiss"),
      press("remote.ambiguous.refresh")
    ]
  );
  h.update(|s| s.remote.ambiguous = None);
  assert!(!h.drawn("remote-ambiguous-dialog"));
}

#[gpui::test]
fn dialogs_stack_in_react_order(cx: &mut TestAppContext) {
  let (mut h, _) = remote_harness(
    cx,
    FakeHost::default(),
    serde_json::json!({
      "setup": { "allowUserManaged": true, "cloud": {} },
      "ambiguous": "Connection reset",
    }),
  );
  // The later dialog is above the earlier: its buttons take the click.
  h.click("remote-ambiguous-dismiss");
  assert_eq!(h.sent(), vec![press("remote.ambiguous.dismiss")]);
}

#[gpui::test]
fn user_managed_form_suggests_ssh_config_aliases(cx: &mut TestAppContext) {
  let (mut h, host) = remote_harness(cx, own_vm_host(), setup_route(true, serde_json::json!({})));
  h.click("remote-setup-own-vm");
  assert!(!h.drawn("remote-hostname-suggestion-0"));
  type_into(&mut h, "remote-hostname", "DEV");
  h.cx.run_until_parked();
  // Aliases that contain what is typed, whatever the case; "staging" does not.
  assert!(h.drawn("remote-hostname-suggestion-0"));
  assert!(!h.drawn("remote-hostname-suggestion-1"));
  h.click("remote-hostname-suggestion-0");
  // Picking one only fills the field: nothing is probed or trusted.
  assert!(!h.drawn("remote-hostname-suggestion-0"));
  type_into(&mut h, "remote-username", "me");
  h.click("remote-continue");
  h.cx.run_until_parked();
  assert_eq!(host.writes(), ["probe devbox:22"]);
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
  fn queue(
    &self,
    full_name: &str,
    branch: &str,
    action: crate::core::account::QueueAction,
  ) -> Result<(), crate::core::supabase::ApiError> {
    self.record(format!("queue {full_name} {branch} {action:?}"));
    if self.failing.contains(&"queue_pro") {
      return Err(crate::core::supabase::ApiError::new(
        "pro_required",
        "Needs Pro",
      ));
    }
    Ok(())
  }

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

/// `open-non-git-folder.test.tsx`: the window model offers to initialize
/// a folder without `.git` first (see the window tests); one that still
/// has none toasts, creates no `.treq` and remembers nothing.
#[test]
fn a_folder_that_is_not_a_repository_is_not_opened() {
  let dir = tempfile::TempDir::new().unwrap();
  let path = dir.path().to_str().unwrap();
  let host = FakeHost::default();
  let mut shown = Vec::new();

  onboarding::open_repo(&host, path, |p| shown.push(p.to_string()));

  assert_eq!(host.calls(), ["notify toast Not a Git Repository"]);
  assert!(shown.is_empty(), "not the window's repository");
  assert!(!dir.path().join(".treq").exists());
  assert!(!dir.path().join(".git").exists());
}

#[test]
fn opening_a_repository_remembers_it_and_reports_it_open() {
  let dir = tempfile::TempDir::new().unwrap();
  std::fs::create_dir(dir.path().join(".git")).unwrap();
  let path = dir.path().to_str().unwrap();
  let host = FakeHost::default();
  let mut shown = Vec::new();

  onboarding::open_repo(&host, path, |p| shown.push(p.to_string()));

  assert_eq!(
    host.calls(),
    [
      format!("set last_opened_repo_path={path}"),
      "notify repoOpened".into(),
      format!("notify toast Opened {}", path.rsplit('/').next().unwrap()),
    ]
  );
  assert_eq!(shown, [path]);
}

mod allocs;
mod changes_large;
mod commit_diffs;
mod commits_large;
mod conflicts;
mod file_browser;
mod fuzz;
mod github;
mod idle;
mod kept_tabs;
mod linear_view;
mod logs_explorer;
mod merge_queue;
mod open_workspaces;
mod pr_review;
mod prefetch;
mod review_comments;
mod stack_panel;
mod tab_badges;
mod testing_library;
mod text_editing;
mod virtual_chaos;
mod virtual_lists;
mod workspace_actions;
mod workspace_dialogs;
