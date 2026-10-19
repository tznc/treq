use std::{
  cell::RefCell,
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

type Sink = Arc<dyn Fn(String) + Send + Sync>;

/// The backend, as the screens see it, recording what they asked of it.
#[derive(Default)]
struct FakeHost {
  remote_ssh: bool,
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
  description: String,
  /// What the Changes tab reads.
  working: Option<crate::jj::JjRevisionDiff>,
  /// The kept review's comments JSON.
  review: Mutex<Option<String>>,
  /// The pull request's review threads and the review agent's comments.
  threads: Vec<crate::github::GhReviewThread>,
  agent: Mutex<Vec<crate::local_db::AgentReviewComment>>,
  /// What the Logs tab reads.
  logs: Vec<crate::core::checks_logs::LogRecordView>,
  workflows: Vec<crate::core::checks::WorkflowInfo>,
  chats: Vec<crate::core::agent_chat::AgentChat>,
  sql: Option<Result<crate::core::checks_logs::SqlResult, String>>,
  /// What the terminal pane reads, and the PTYs it started.
  sessions: Vec<crate::local_db::Session>,
  ptys: Mutex<Vec<String>>,
  /// PTY output listeners, by PTY id.
  sinks: Mutex<Vec<(String, Sink)>>,
  /// Whether confirmations are cancelled.
  decline: bool,
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
  calls: Mutex<Vec<String>>,
}

impl FakeHost {
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

impl onboarding::OnboardingHost for FakeHost {
  fn remote_ssh_enabled(&self) -> bool {
    self.remote_ssh
  }

  fn pick_repo(&self) {
    self.record("pick_repo".into());
  }
}

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
    self.loaded.clone()
  }

  fn pr_cache(&self, _: &str, branch: &str) -> workspace_data::PrCache {
    self.record(format!("read pr {branch}"));
    self.pr.clone()
  }

  fn push(&self, _: &str, workspace_id: Option<i64>) -> workspace_data::Outcome {
    self.record(format!("push {workspace_id:?}"));
    workspace_data::Outcome::ok("Pushed to remote", "Changes pushed successfully".into())
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
    false
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
    let working = self.working.as_ref().ok_or("no changes")?;
    Ok(working.uncommitted_files.clone())
  }

  fn workspace_diff(
    &self,
    _: &str,
    workspace_id: i64,
  ) -> Result<crate::jj::JjRevisionDiff, String> {
    self.record(format!("read workspace diff {workspace_id}"));
    self.working.clone().ok_or_else(|| "no changes".into())
  }

  fn file_hunks(
    &self,
    _: &str,
    _: Option<i64>,
    paths: Vec<String>,
  ) -> Result<crate::core::WorkspaceFileHunksBatch, String> {
    self.record(format!("read hunks {paths:?}"));
    let working = self.working.as_ref().ok_or("no changes")?;
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
    self.record(format!("write {id} {data:?}"));
    Ok(())
  }

  fn resize_pty(&self, _: &str, _: u16, _: u16) -> Result<(), String> {
    Ok(())
  }

  fn close_pty(&self, id: &str) -> Result<(), String> {
    self.record(format!("close {id}"));
    self.ptys.lock().unwrap().retain(|p| p != id);
    Ok(())
  }

  fn listen(&self, id: &str, sink: Box<dyn Fn(String) + Send + Sync>) -> terminal_data::Listening {
    self
      .sinks
      .lock()
      .unwrap()
      .push((id.to_string(), Arc::from(sink)));
    terminal_data::Listening::new(|| {})
  }

  fn sessions(&self, _: &str) -> Result<Vec<crate::local_db::Session>, String> {
    Ok(self.sessions.clone())
  }

  fn supporting_repos(&self, _: &str) -> Vec<String> {
    Vec::new()
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
}

impl FakeHost {
  /// Sends `data` as the PTY's output.
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
      sink(data.to_string());
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
    Ok(Vec::new())
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
      .ok_or_else(|| "no file diff".into())
  }

  fn abandon(&self, _: &str, _: i64, change_id: &str) -> Result<String, String> {
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

  fn revert(&self, _: &str, _: i64, change_id: &str) -> Result<(), String> {
    self.record(format!("revert {change_id}"));
    Ok(())
  }

  fn shift_to_now(&self, _: &str, workspace_id: i64) -> Result<(), String> {
    self.record(format!("shift to now {workspace_id}"));
    Ok(())
  }

  fn commit_description(&self, _: &str, _: i64, change_id: &str) -> Result<String, String> {
    self.record(format!("read description {change_id}"));
    Ok(self.description.clone())
  }

  fn describe(&self, _: &str, _: i64, change_id: &str, description: &str) -> Result<(), String> {
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

  fn confirm(&self, title: &str, _: &str) -> bool {
    self.record(format!("confirm {title}"));
    !self.decline
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
    let (screen, cx) = cx.add_window_view(|_, _| Screen::new(snapshot, sink, popups, None, host));
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
      Some(PopupRequest::ShowDropdown { entries, .. } | PopupRequest::ShowMenu { entries, .. }) => {
        entries.clone()
      }
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

  fn click(&mut self, selector: &'static str) {
    self.cx.run_until_parked();
    let bounds = self
      .cx
      .debug_bounds(selector)
      .unwrap_or_else(|| panic!("{selector} is not drawn"));
    self.cx.simulate_click(bounds.center(), Modifiers::none());
  }

  fn sent(&self) -> Vec<ScreenAction> {
    self.sent.borrow().clone()
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
fn onboarding_runs_its_own_actions(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    installed: vec![("git", false), ("gh", true)],
    ..FakeHost::default()
  });
  let mut h = Harness::with_host(cx, onboarding(), host.clone());
  // The check ran as the screen opened.
  assert!(h.drawn("install-git"));
  assert!(!h.drawn("install-gh"));
  h.click("onboarding-open-repo");
  h.click("prerequisites-recheck");
  h.click("install-git");
  // Only the remote dialog is still React's.
  h.click("onboarding-open-remote");
  assert_eq!(
    host.calls(),
    [
      "check",
      "pick_repo",
      "check",
      "open https://git-scm.com/downloads"
    ]
  );
  assert_eq!(h.sent(), vec![ScreenAction::OpenRemote]);
}

#[gpui::test]
fn onboarding_labels_the_remote_button_by_preview(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    remote_ssh: true,
    ..FakeHost::default()
  });
  let mut h = Harness::with_host(cx, onboarding(), host);
  assert!(h.drawn("onboarding-open-remote"));
  let label = h.screen.read_with(&h.cx, |screen, cx| {
    screen
      .onboarding
      .as_ref()
      .map(|view| view.read(cx).remote_label.clone())
  });
  assert_eq!(label.as_deref(), Some("Open via SSH"));
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
  let host = Arc::new(FakeHost::with_settings(&[
    ("/repo:ignore_generated_treq_paths", "true"),
    ("/repo:auto_review_trigger", "on-push"),
  ]));
  let mut h = Harness::with_host(cx, settings(), host.clone());
  let screen = drawn_settings(&mut h);
  let repo = screen.repository.expect("repository tab");
  assert!(!repo.loading);
  assert_eq!(repo.current_branch.as_deref(), Some("main"));
  assert_eq!(repo.fields.branch_name_pattern, "treq/{name}");
  assert!(repo.fields.ignore_generated_agent_files);
  // The trigger's old name reads as the new one.
  assert_eq!(repo.fields.auto_review_trigger, "on-pull");
  assert_eq!(repo.model_options[0].label, "Use Application Default");

  h.click("settings-tab-application");
  h.click("settings-auto-push");
  h.click("settings-ignore-generated");
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
      "set repo /repo branch_name_pattern=treq/{name} included_copy_files= default_model= default_agent= auto_push=true ignore_generated_treq_paths=false review_prompt= review_agent= auto_review_trigger=on-pull",
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
      "notify toast Settings Saved",
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
    "checks": {"status": "passed", "treq": "3/3", "github": {"label": "GH 4/4", "state": "success"}},
    "mergeQueue": null,
    "merge": {"passing": true, "disabled": false, "tooltip": null},
    "details": true,
    "more": {"disabled": false},
    "title": "Add login"
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
    "tabRow": true,
    "retargeting": false,
    "refreshing": false,
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
    checks: Some(workspace_data::ChecksSummary {
      workflows: vec!["a.yml".into(), "b.yml".into(), "c.yml".into()],
      latest: ["a.yml", "b.yml", "c.yml"]
        .map(|f| (f.to_string(), "passed".to_string()))
        .to_vec(),
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
    "workspace-target",
    "workspace-view-pr",
    "workspace-view-pr-web",
    "workspace-checks",
    "workspace-merge",
    "workspace-details",
    "workspace-more",
  ] {
    h.click(selector);
  }
  h.click("workspace-linear");
  h.cx.run_until_parked();
  assert_eq!(
    h.sent(),
    vec![
      press("workspace.target"),
      press("workspace.viewPr"),
      press("workspace.checks"),
      press("workspace.merge"),
      press("workspace.details"),
      press("workspace.more"),
    ]
  );
  assert_eq!(
    host.writes(),
    vec![
      "open https://github.com/tznc/treq/pull/42",
      "open https://linear.app/t/ENG-12",
    ]
  );
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
  let reads = host
    .calls()
    .iter()
    .filter(|c| c.starts_with("read workspace"))
    .count();
  assert_eq!(reads, 2, "it reads the workspace again after syncing");
}

#[gpui::test]
fn workspace_header_reads_again_when_react_refreshes(cx: &mut TestAppContext) {
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
    .filter(|c| c.starts_with("read workspace"))
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
fn workspace_leaves_the_tab_row_to_react_without_one(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, feature_host(), workspace_route());
  assert!(h.drawn("workspace-tab-row"));
  h.update(|s| match &mut s.view {
    View::Workspace(route) => route.tab_row = false,
    _ => unreachable!(),
  });
  assert!(!h.drawn("workspace-tab-row"));
  assert!(h.drawn("workspace-header"));
  assert!(h.drawn("workspace-frame"));
}

#[gpui::test]
fn workspace_frame_fills_the_space_below(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, feature_host(), workspace_route());
  h.cx.run_until_parked();
  let header = h.cx.debug_bounds("workspace-header").expect("header");
  let row = h.cx.debug_bounds("workspace-tab-row").expect("tab row");
  let frame = h.cx.debug_bounds("workspace-frame").expect("frame");
  assert_eq!(row.top(), header.bottom());
  assert_eq!(frame.top(), row.bottom());
  assert!(frame.size.height > gpui::px(100.));
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

/// What React sends of its auth store and cloud workspace flows, signed
/// out, with `patch` merged in.
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

fn account_route(tab: &str, account: serde_json::Value) -> ScreenSnapshot {
  serde_json::from_value(serde_json::json!({
    "kind": "settings",
    "dark": false,
    "tab": tab,
    "repoPath": "/repo",
    "currentBranch": "main",
    "account": account,
  }))
  .expect("parse")
}

#[gpui::test]
fn account_signs_in_through_the_browser(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::default());
  let route = account_route(
    "account",
    account_state(serde_json::json!({ "signInError": "token expired" })),
  );
  let mut h = Harness::with_host(cx, route, host.clone());
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
  let route = account_route(
    "account",
    account_state(serde_json::json!({ "loading": true })),
  );
  let mut h = Harness::new(cx, route);
  assert_eq!(
    drawn_settings(&mut h).account,
    Some(settings::AccountTab::Loading)
  );
  h.update(|s| {
    if let View::Settings(route) = &mut s.view {
      route.account =
        serde_json::from_value(account_state(serde_json::json!({ "signingIn": true })))
          .expect("account");
    }
  });
  assert_eq!(
    drawn_settings(&mut h).account,
    Some(settings::AccountTab::SigningIn)
  );
  assert!(h.drawn("account-signing-in"));
}

#[gpui::test]
fn account_hands_a_pasted_callback_token_to_react(cx: &mut TestAppContext) {
  let route = account_route("account", account_state(serde_json::json!({})));
  let mut h = Harness::new(cx, route);
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
  settings_act(&mut h, field("account.callbackUrl", "rawtoken".into()));
  h.click("account-dev-callback-go");
  assert_eq!(
    h.sent(),
    [
      field("account.devCallback", "abc".into()),
      field("account.devCallback", "rawtoken".into()),
    ]
  );
}

#[gpui::test]
fn account_shows_the_plan_and_signs_out(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::default());
  let mut user = ada();
  user["avatarUrl"] = "https://example.com/ada.png".into();
  let route = account_route(
    "account",
    account_state(serde_json::json!({ "user": user, "subscription": pro() })),
  );
  let mut h = Harness::with_host(cx, route, host.clone());
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
      "open http://localhost:3001/dashboard"
    ]
  );
  assert_eq!(h.sent(), [press("account.signOut")]);
}

#[gpui::test]
fn account_offers_pro_on_the_free_plan(cx: &mut TestAppContext) {
  let route = account_route(
    "account",
    account_state(serde_json::json!({
      "user": ada(),
      "subscription": { "plan": "free", "status": "canceled", "currentPeriodEnd": null },
    })),
  );
  let mut h = Harness::new(cx, route);
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

fn signed_in_with_cloud(cloud: serde_json::Value) -> ScreenSnapshot {
  account_route(
    "account",
    account_state(serde_json::json!({ "user": ada(), "subscription": pro(), "cloud": cloud })),
  )
}

fn cloud_card(h: &mut Harness) -> settings::CloudCard {
  match drawn_settings(h).account {
    Some(settings::AccountTab::SignedIn(signed_in)) => signed_in.cloud.expect("cloud card"),
    other => panic!("signed in: {other:?}"),
  }
}

#[gpui::test]
fn cloud_workspace_creates_through_react(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, signed_in_with_cloud(cloud(serde_json::json!({}))));
  let card = cloud_card(&mut h);
  assert_eq!(card.instance, None);
  assert_eq!(
    card.create.as_ref().map(|c| c.label.as_str()),
    Some("Create cloud workspace")
  );
  h.click("cloud-create");
  assert_eq!(h.sent(), [press("cloud.provision")]);

  h.update(|s| {
    if let View::Settings(route) = &mut s.view {
      route.account = serde_json::from_value(account_state(serde_json::json!({
        "user": ada(),
        "cloud": cloud(serde_json::json!({ "provisioningStage": "Requesting provisioning..." })),
      })))
      .expect("account");
    }
  });
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
  let mut h = Harness::new(cx, signed_in_with_cloud(ready));
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
  assert_eq!(
    h.sent(),
    [press("cloud.openRepositories"), press("cloud.delete")]
  );
}

#[gpui::test]
fn cloud_workspace_repairs_and_upsells_pro(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::default());
  let degraded = cloud(serde_json::json!({
    "instance": { "status": "degraded", "providerResourceId": "m-1", "diskQuotaGb": 5 },
    "provisioningError": "[pro_required] Cloud workspaces need Pro.\nhttp 402",
  }));
  let mut h = Harness::with_host(cx, signed_in_with_cloud(degraded), host.clone());
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
  assert_eq!(h.sent(), [press("cloud.repair")]);
  assert_eq!(host.writes(), ["open http://localhost:3001/dashboard"]);

  h.update(|s| {
    if let View::Settings(route) = &mut s.view {
      route.account = serde_json::from_value(account_state(serde_json::json!({
        "user": ada(),
        "cloud": cloud(serde_json::json!({
          "instance": { "status": "failed", "providerResourceId": null, "diskQuotaGb": 5 },
          "provisioningError": "[http_502] exec failed\nstack",
        })),
      })))
      .expect("account");
    }
  });
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
  let route = account_route("integrations", account_state(serde_json::json!({})));
  let mut h = Harness::with_host(cx, route, host.clone());
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
  let github = serde_json::json!({
    "loading": false,
    "failed": false,
    "repositories": [
      { "id": 1, "fullName": "acme/public", "private": false },
      { "id": 2, "fullName": "acme/private", "private": true },
    ],
  });
  let route = account_route(
    "integrations",
    account_state(serde_json::json!({ "user": ada(), "github": github })),
  );
  let mut h = Harness::with_host(cx, route, host.clone());
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
  let route = account_route(
    "integrations",
    account_state(serde_json::json!({ "user": ada(), "subscription": subscription })),
  );
  let h = Harness::with_host(cx, route, host.clone());
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
fn linear_connects_via_oauth_through_react_on_pro(cx: &mut TestAppContext) {
  let (mut h, host) = linear_harness(cx, true, Some("lin_api_1"));
  let linear = linear_section(&mut h);
  assert!(linear.oauth);
  assert!(linear.key_priority_note);
  assert_eq!(
    linear.oauth_description,
    "API key takes priority, but you can also connect via OAuth"
  );
  h.click("linear-oauth");
  assert_eq!(h.sent(), [press("linear.oauth")]);
  assert!(host.writes().is_empty());
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
    "workspace-frame",
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
  h.click("agent-task-input");
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
  let created = fields(&h, "code.sessionCreated");
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
  h.click("agent-task-input");
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
  assert_eq!(fields(&h, "code.sessionCreated")[0]["workspaceId"], 9);
}

#[gpui::test]
fn code_tab_suggests_files_for_mentions(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, code_host(), code_route());
  h.click("agent-task-input");
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
    "workspace-frame",
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
fn commits_tab_abandons_once_confirmed(cx: &mut TestAppContext) {
  let mut declined = commits_host();
  declined.decline = true;
  let (mut h, host) = workspace_harness(cx, declined, commits_route());
  h.click("commit-row-a");
  h.click("delete-commit-a");
  h.cx.run_until_parked();
  assert!(host.writes().contains(&"confirm Delete Commit".to_string()));
  assert!(!host.writes().iter().any(|w| w.starts_with("abandon")));
}

#[gpui::test]
fn commits_tab_removes_an_abandoned_commit(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, commits_host(), commits_route());
  h.click("commit-row-a");
  h.click("delete-commit-a");
  h.cx.run_until_parked();
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
  let created = fields(&h, "code.sessionCreated");
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
  h.click("resolve-conflicts-submit");
  h.cx.run_until_parked();
  assert!(
    host
      .writes()
      .contains(&r#"create session Some(7) "Resolve a""#.to_string()),
    "{:?}",
    host.writes()
  );
  let created = fields(&h, "code.sessionCreated");
  assert_eq!(created[0]["workspacePath"], "/r/cwd");
  assert_eq!(created[0]["pendingPrompt"], "resolve: ");
  assert!(!h.drawn("resolve-conflicts-dialog"));
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

#[gpui::test]
fn changes_tab_asks_for_a_commit_message(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  h.click("commit-button");
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

#[gpui::test]
fn changes_tab_offers_the_browser_view(cx: &mut TestAppContext) {
  let mut route = changes_route();
  if let View::Workspace(r) = &mut route.view {
    r.review_switcher = true;
  }
  let (mut h, _) = workspace_harness(cx, changes_host(), route);
  assert!(h.drawn("review-switcher"));
  h.update(|s| {
    if let View::Workspace(r) = &mut s.view {
      r.review_switcher = false;
    }
  });
  assert!(!h.drawn("review-switcher"));
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
  let sent = fields(&h, "changes.review");
  assert_eq!(sent.len(), 1);
  assert_eq!(sent[0]["mode"], "plan");
  assert_eq!(sent[0]["useIdleAgent"], false);
  let markdown = sent[0]["markdown"].as_str().unwrap_or_default();
  assert!(
    markdown.contains("Nit") && markdown.contains("a.rs"),
    "{markdown}"
  );
  assert!(!h.drawn("review-action-bar"));
  assert!(!h.drawn("finish-review-popover"));
  assert!(host.writes().contains(&"clear review 7".to_string()));
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
      let search = &workspace.changes.as_ref().expect("changes").read(cx).search;
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
  let (mut h, _) = workspace_harness(cx, logs_host(), logs_route(None));
  assert!(!h.drawn("log-inspect-panel"));
  h.click("repo-log-line");
  assert!(h.drawn("log-inspect-panel"));
  assert!(h.drawn("selection-count"));
  h.click("send-to-agent");
  h.cx.run_until_parked();
  let sent = fields(&h, "logs.sendToAgent");
  assert_eq!(sent.len(), 1);
  let prompt = sent[0].as_str().unwrap_or_default();
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
  let (mut h, _) = workspace_harness(cx, host, logs_route(None));
  h.click("logs-explorer-view");
  h.click("run-query");
  h.cx.run_until_parked();
  assert!(h.drawn("sql-results"));
  h.click("send-resultset-to-agent");
  h.cx.run_until_parked();
  let sent = fields(&h, "logs.sendToAgent");
  assert_eq!(sent.len(), 1);
  assert!(
    sent[0]
      .as_str()
      .unwrap_or_default()
      .contains("Result (1 row):"),
    "{sent:?}"
  );
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
fn terminal_pane_starts_collapsed_and_expands(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  assert!(h.drawn("workspace-terminal-pane"));
  assert!(h.drawn("terminal-expand"));
  assert!(!h.drawn("terminal-scroll-container"));
  h.click("terminal-expand");
  assert!(h.drawn("terminal-scroll-container"));
  assert!(h.drawn("terminal-collapse"));
  // No terminal yet: nothing to maximize.
  assert!(!h.drawn("terminal-maximize"));
}

#[gpui::test]
fn terminal_pane_opens_the_active_agent_session(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(
    &mut h,
    serde_json::json!({
      "activeSessionId": 3,
      "pending": [{ "sessionId": 3, "pendingPrompt": "fix it", "permissionMode": "plan", "agent": "codex" }],
    }),
  );
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
  // Cmd+W closes the active agent; the shell next to it takes focus.
  h.cx.simulate_keystrokes("cmd-w");
  h.cx.run_until_parked();
  assert!(!h.drawn("terminal-column-agent"));
  assert!(host.calls().contains(&format!("close {}", agent_pty())));
  assert!(h.sent().contains(&field("terminal.closeSession", 3.into())));
  h.cx.simulate_keystrokes("cmd-w");
  h.cx.run_until_parked();
  assert!(!h.drawn("terminal-column-shell"));
  // Closing the last terminal collapses the pane.
  assert!(h.drawn("terminal-expand"));
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
    Ok(crate::core::RenameWorkspaceResult {
      success: !taken,
      message: if taken {
        format!("Branch '{branch}' already exists")
      } else {
        String::new()
      },
      workspace: None,
      updated_children_ids: Vec::new(),
    })
  }
}

mod workspace_dialogs;
