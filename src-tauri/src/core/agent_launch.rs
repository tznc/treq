//! Starting agent sessions: the prompt dialog's task, issue workspaces,
//! reviews, deep links and the start data a new session's terminal reads.
//!
//! Every path that starts an agent on the user's behalf ends in a
//! [`SessionStart`]: the session row, plus what its terminal sends once the
//! agent is up (the prompt, the permission mode, the agent). The window's
//! terminal pane reads that start data from [`pending_session`] when it
//! opens the session, so it lives here rather than in a view.

use std::{
  collections::{HashMap, HashSet},
  sync::{Mutex, OnceLock},
};

use serde::{Deserialize, Serialize};

use crate::lock_ext::LockExt;

/// Every agent treq can launch.
pub const AGENTS: [(&str, &str); 4] = [
  ("claude", "Claude"),
  ("codex", "Codex"),
  ("cursor", "Cursor"),
  ("copilot", "Copilot"),
];

/// `toAgentKind`: the agent id, when treq knows it.
pub fn agent_kind(value: Option<&str>) -> Option<String> {
  let value = value?;
  AGENTS
    .iter()
    .find(|(id, _)| *id == value)
    .map(|(id, _)| (*id).to_string())
}

fn agent_label(agent: Option<&str>) -> &'static str {
  AGENTS
    .iter()
    .find(|(id, _)| Some(*id) == agent)
    .map_or(AGENTS[0].1, |(_, label)| label)
}

/// Where an attached issue lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum IssueSource {
  #[default]
  Github,
  Linear,
}

impl IssueSource {
  fn label(self) -> &'static str {
    match self {
      IssueSource::Github => "GitHub",
      IssueSource::Linear => "Linear",
    }
  }
}

/// `IssueAttachment`: an issue attached to an agent prompt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct IssueAttachment {
  pub source: IssueSource,
  /// What the source's backend needs: a GitHub issue number as text, or a
  /// Linear issue id.
  pub id: String,
  /// What the user sees: `#42`, `ENG-101`.
  pub key: String,
  pub url: String,
  pub title: String,
  /// Also open workspaces for the issue's sub-items.
  pub include_sub_items: bool,
  pub sub_item_ids: Vec<String>,
}

/// `formatPromptWithIssue`.
pub fn format_prompt_with_issue(text: &str, issue: &IssueAttachment) -> String {
  let trimmed = text.trim();
  let reference = format!("{} issue {}", issue.source.label(), issue.key);
  if trimmed.is_empty() {
    let title = issue.title.trim();
    let title = if title.is_empty() {
      String::new()
    } else {
      format!(": {title}")
    };
    return format!("Address {reference}{title}\n\n{}", issue.url);
  }
  format!("{trimmed}\n\n{reference}: {}", issue.url)
}

/// A new session's name from its prompt: at most 50 characters.
pub fn session_name(prompt: &str) -> String {
  if prompt.chars().count() > 50 {
    format!("{}...", prompt.chars().take(47).collect::<String>())
  } else {
    prompt.to_string()
  }
}

/// A started session, as React's `SessionCreationInfo`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SessionStart {
  pub session_id: i64,
  pub session_name: String,
  pub workspace_id: Option<i64>,
  /// Where the agent runs, when not its workspace's own directory.
  pub workspace_path: Option<String>,
  pub repo_path: String,
  pub pending_prompt: Option<String>,
  /// `plan` or `acceptEdits`.
  pub permission_mode: Option<String>,
  pub agent: Option<String>,
}

impl SessionStart {
  pub fn pending(&self) -> PendingSession {
    PendingSession {
      pending_prompt: self.pending_prompt.clone(),
      permission_mode: self.permission_mode.clone(),
      agent: self.agent.clone(),
      workspace_path: self.workspace_path.clone(),
    }
  }
}

/// What a new session's terminal starts with (`pendingSessionData`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct PendingSession {
  pub pending_prompt: Option<String>,
  pub permission_mode: Option<String>,
  pub agent: Option<String>,
  pub workspace_path: Option<String>,
}

impl PendingSession {
  pub fn is_empty(&self) -> bool {
    self == &PendingSession::default()
  }
}

/// A toast a launch asks for; `warning` false is a success.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toast {
  pub title: String,
  pub description: String,
  pub warning: bool,
}

/// What launching needs from the backend; faked in tests.
pub trait LaunchStore {
  fn create_session(
    &self,
    repo_path: &str,
    workspace_id: Option<i64>,
    name: &str,
  ) -> Result<i64, String>;
  /// How many sessions run in the workspace (`None`: the home repository).
  fn session_count(&self, repo_path: &str, workspace_id: Option<i64>) -> usize;
  fn repo_setting(&self, repo_path: &str, key: &str) -> Option<String>;
  fn app_setting(&self, key: &str) -> Option<String>;
  fn set_repo_setting(&self, repo_path: &str, key: &str, value: &str) -> Result<(), String>;
  fn add_prompt_history(
    &self,
    repo_path: &str,
    workspace_id: Option<i64>,
    session_id: i64,
    prompt: &str,
    agent: Option<&str>,
  ) -> Result<(), String>;
  /// Opens one issue's workspace, creating it when needed: its id, and
  /// whether it was created.
  fn open_issue_workspace(
    &self,
    repo_path: &str,
    source: IssueSource,
    id: &str,
    title: &str,
    url: &str,
  ) -> Result<(i64, bool), String>;
}

/// `IssueWorkspace`: the parent issue's workspace and how its sub-items went.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IssueWorkspace {
  pub workspace_id: i64,
  /// Sub-items opened: id, workspace id, created.
  pub sub_item_results: Vec<(String, i64, bool)>,
  /// Sub-items that failed: id, error.
  pub sub_item_failures: Vec<(String, String)>,
}

/// The backend opens or creates one workspace per call, so kickoffs run
/// one at a time: a second waits until the first opened every sub-item.
static KICKOFF: Mutex<()> = Mutex::new(());

/// `openOrCreateIssueWorkspace`: the attached issue's workspace, then each
/// sub-item's in turn when asked. A failed sub-item is reported and
/// skipped; a failed parent fails the whole.
pub fn open_or_create_issue_workspace(
  store: &dyn LaunchStore,
  repo_path: &str,
  issue: &IssueAttachment,
) -> Result<IssueWorkspace, String> {
  let _one_at_a_time = KICKOFF.lock_or_recover();
  let (workspace_id, _) =
    store.open_issue_workspace(repo_path, issue.source, &issue.id, &issue.title, &issue.url)?;
  let mut result = IssueWorkspace {
    workspace_id,
    ..Default::default()
  };
  if issue.include_sub_items {
    for id in &issue.sub_item_ids {
      match store.open_issue_workspace(repo_path, issue.source, id, "", "") {
        Ok((workspace, created)) => result
          .sub_item_results
          .push((id.clone(), workspace, created)),
        Err(error) => result.sub_item_failures.push((id.clone(), error)),
      }
    }
  }
  Ok(result)
}

/// `subItemToasts`: only the parent's workspace opens, so say what
/// happened to the sub-items'.
pub fn sub_item_toasts(issue: &IssueWorkspace) -> Vec<Toast> {
  let mut toasts = Vec::new();
  if !issue.sub_item_results.is_empty() {
    let created = issue.sub_item_results.iter().filter(|r| r.2).count();
    let opened = issue.sub_item_results.len() - created;
    toasts.push(Toast {
      title: "Sub-issue workspaces ready".into(),
      description: if opened > 0 {
        format!("Created {created} and opened {opened} existing.")
      } else {
        format!("Created {created}.")
      },
      warning: false,
    });
  }
  if !issue.sub_item_failures.is_empty() {
    let count = issue.sub_item_failures.len();
    toasts.push(Toast {
      title: format!(
        "{count} sub-issue workspace{} failed",
        if count == 1 { "" } else { "s" }
      ),
      description: issue
        .sub_item_failures
        .iter()
        .map(|f| f.1.as_str())
        .collect::<Vec<_>>()
        .join("; "),
      warning: true,
    });
  }
  toasts
}

/// What the prompt dialog's Edit or Plan starts (`GpuiAgentPromptStart`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TaskRequest {
  pub repo_path: String,
  pub workspace_id: Option<i64>,
  pub text: String,
  pub issue: Option<IssueAttachment>,
  pub agent: String,
  pub mode: String,
  pub save_as_repo_default: bool,
}

/// `startAgentTask`: saves the agent as the repository's default when
/// asked, opens the attached issue's workspace and creates the session.
/// `None` when there is nothing to send.
pub fn start_agent_task(
  store: &dyn LaunchStore,
  task: &TaskRequest,
) -> Result<Option<(SessionStart, Vec<Toast>)>, String> {
  let trimmed = task.text.trim();
  if trimmed.is_empty() && task.issue.is_none() {
    return Ok(None);
  }
  let prompt = match &task.issue {
    Some(issue) => format_prompt_with_issue(trimmed, issue),
    None => trimmed.to_string(),
  };
  if task.save_as_repo_default {
    store.set_repo_setting(&task.repo_path, "default_agent", &task.agent)?;
  }
  let name = session_name(&prompt);
  let (workspace_id, toasts) = match &task.issue {
    Some(issue) => {
      let opened = open_or_create_issue_workspace(store, &task.repo_path, issue)?;
      (Some(opened.workspace_id), sub_item_toasts(&opened))
    }
    None => (task.workspace_id, Vec::new()),
  };
  let session_id = store.create_session(&task.repo_path, workspace_id, &name)?;
  Ok(Some((
    SessionStart {
      session_id,
      session_name: name,
      workspace_id,
      workspace_path: None,
      repo_path: task.repo_path.clone(),
      pending_prompt: Some(prompt),
      permission_mode: Some(task.mode.clone()),
      agent: Some(task.agent.clone()),
    },
    toasts,
  )))
}

/// A session treq starts on its own behalf: a review, a deep link, logs or
/// a comment sent to an agent (`launchAgentSession`, `startAgentSession`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LaunchRequest {
  pub repo_path: String,
  /// `None` starts it in the home repository.
  pub workspace_id: Option<i64>,
  pub prompt: Option<String>,
  pub mode: Option<String>,
  pub agent: Option<String>,
  /// Default: `<Agent> <n>`, numbered within the workspace.
  pub session_name: Option<String>,
}

/// Creates the session `request` asks for.
pub fn launch_session(
  store: &dyn LaunchStore,
  request: &LaunchRequest,
) -> Result<SessionStart, String> {
  let repo = &request.repo_path;
  let name = request.session_name.clone().unwrap_or_else(|| {
    let index = store.session_count(repo, request.workspace_id) + 1;
    format!("{} {index}", agent_label(request.agent.as_deref()))
  });
  let session_id = store.create_session(repo, request.workspace_id, &name)?;
  Ok(SessionStart {
    session_id,
    session_name: name,
    workspace_id: request.workspace_id,
    workspace_path: None,
    repo_path: repo.clone(),
    pending_prompt: request.prompt.clone(),
    permission_mode: request.mode.clone(),
    agent: request.agent.clone(),
  })
}

/// `resolveDefaultAgent`: the override, else the repository's default
/// agent, else the app's; `None` leaves it to the terminal (Claude).
pub fn resolve_agent(
  store: &dyn LaunchStore,
  repo_path: &str,
  agent: Option<&str>,
) -> Option<String> {
  let non_empty = |v: Option<String>| v.filter(|v| !v.is_empty());
  let chosen = non_empty(agent.map(str::to_string))
    .or_else(|| non_empty(store.repo_setting(repo_path, "default_agent")))
    .or_else(|| non_empty(store.app_setting("default_agent")));
  agent_kind(chosen.as_deref())
}

/// Records the session's prompt in the prompt history, as
/// `handleSessionCreated` does.
pub fn record_prompt(store: &dyn LaunchStore, start: &SessionStart) -> Result<(), String> {
  match &start.pending_prompt {
    Some(prompt) if !prompt.is_empty() => store.add_prompt_history(
      &start.repo_path,
      start.workspace_id,
      start.session_id,
      prompt,
      start.agent.as_deref(),
    ),
    _ => Ok(()),
  }
}

// The terminal pane's start data.

/// Start data by window, then by pane session id.
type PendingByWindow = HashMap<String, HashMap<i64, PendingSession>>;

fn pending_sessions() -> &'static Mutex<PendingByWindow> {
  static PENDING: OnceLock<Mutex<PendingByWindow>> = OnceLock::new();
  PENDING.get_or_init(Default::default)
}

/// Keeps what window `window`'s terminal starts session `pane_id` with.
pub fn remember_pending(window: &str, pane_id: i64, pending: PendingSession) {
  if pending.is_empty() {
    return;
  }
  pending_sessions()
    .lock_or_recover()
    .entry(window.to_string())
    .or_default()
    .insert(pane_id, pending);
}

/// What window `window`'s terminal starts session `pane_id` with, if
/// anything. Kept, like React's map, for as long as the window lives.
pub fn pending_session(window: &str, pane_id: i64) -> Option<PendingSession> {
  pending_sessions()
    .lock_or_recover()
    .get(window)
    .and_then(|m| m.get(&pane_id))
    .cloned()
}

/// Drops window `window`'s start data once it closes.
pub fn forget_window(window: &str) {
  pending_sessions().lock_or_recover().remove(window);
}

fn repo_hash(repo_path: &str) -> u64 {
  repo_path
    .chars()
    .fold(0u64, |hash, c| (hash * 31 + c as u64) % 2_147_483_647)
}

fn base36(mut n: u64) -> String {
  const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
  if n == 0 {
    return "0".into();
  }
  let mut out = Vec::new();
  while n > 0 {
    out.push(DIGITS[(n % 36) as usize]);
    n /= 36;
  }
  out.reverse();
  String::from_utf8(out).unwrap_or_default()
}

/// `agentPtySessionId`: an agent session's PTY id, unique across
/// repositories since PTYs are shared by every window.
pub fn agent_pty_id(repo_path: &str, session_id: i64) -> String {
  format!("session-{}-{session_id}", base36(repo_hash(repo_path)))
}

/// `paneSessionId`: the terminal pane shows sessions of the main and
/// supporting repositories, whose ids overlap, so supporting ones are
/// offset by a hash of their path.
pub fn pane_session_id(repo_path: &str, main_repo_path: &str, session_id: i64) -> i64 {
  if repo_path == main_repo_path {
    session_id
  } else {
    (repo_hash(repo_path) as i64 + 1) * 1_000_000 + session_id
  }
}

// Review prompts.

/// `AGENT_REVIEW_TARGET_WORKSPACE_DIFF`.
pub const REVIEW_TARGET: &str = "workspace_diff";
/// `DEFAULT_REVIEW_PROMPT`, as `lib/agent-review-prompt.ts` writes it.
const DEFAULT_REVIEW_PROMPT: &str = include_str!("review_prompt.txt");

/// `workspaceDiffSummary`.
pub fn diff_summary(branch: Option<&str>, files: usize) -> String {
  let files = format!("{files} changed file{}", if files == 1 { "" } else { "s" });
  match branch {
    Some(branch) => format!("the {branch} workspace diff ({files})"),
    None => format!("the current workspace diff ({files})"),
  }
}

/// The repository's prompt when set, else the built-in one, unfilled.
pub fn review_template(custom: Option<&str>) -> &str {
  custom
    .filter(|c| !c.trim().is_empty())
    .unwrap_or(DEFAULT_REVIEW_PROMPT)
}

/// `buildReviewPrompt`: the repository's prompt when set, else the
/// built-in one, with its placeholders filled.
pub fn review_prompt(custom: Option<&str>, target_id: &str, summary: &str) -> String {
  review_prompt_for(custom, REVIEW_TARGET, target_id, summary)
}

/// [`review_prompt`] for another target type, as a PR review's `github_pr`.
pub fn review_prompt_for(
  custom: Option<&str>,
  target_type: &str,
  target_id: &str,
  summary: &str,
) -> String {
  review_template(custom)
    .replace("{target_type}", target_type)
    .replace("{target_id}", target_id)
    .replace("{workspace_id}", target_id)
    .replace("{diff_summary}", summary)
}

/// `autoReviewSummary`: names the operation that started an automatic
/// review.
pub fn auto_review_summary(trigger: &str, branch: Option<&str>) -> String {
  let what = match trigger {
    "on-commit" => "the commit just created",
    "on-rebase" => "the rebase just applied",
    _ => "the changes just pulled from the remote",
  };
  match branch {
    Some(branch) => format!("{what} on the {branch} workspace diff"),
    None => format!("{what} on the current workspace diff"),
  }
}

/// `prepareWorkspaceReview`: the review prompt and agent, from the
/// repository's `review_prompt` and `review_agent`.
pub fn prepare_workspace_review(
  store: &dyn LaunchStore,
  repo_path: &str,
  workspace_id: i64,
  summary: &str,
) -> (String, Option<String>) {
  let custom = store.repo_setting(repo_path, "review_prompt");
  let agent = store
    .repo_setting(repo_path, "review_agent")
    .filter(|a| !a.is_empty());
  (
    review_prompt(custom.as_deref(), &workspace_id.to_string(), summary),
    agent,
  )
}

/// `handleSendAgentReviewCommentToAgent`'s message for one review comment.
/// `working_dir` is cut off the file path.
pub fn review_comment_prompt(
  comment: &crate::local_db::AgentReviewComment,
  working_dir: &str,
) -> String {
  let prefix = format!("{working_dir}/");
  let file = comment
    .file_path
    .strip_prefix(&prefix)
    .unwrap_or(&comment.file_path);
  let range = if comment.start_line == comment.end_line {
    String::new()
  } else {
    format!("-{}", comment.end_line)
  };
  let suggestion = comment
    .suggested_replacement
    .as_ref()
    .map(|s| format!("\nSuggested change:\n```\n{s}\n```\n"))
    .unwrap_or_default();
  format!(
    "Please address this review comment.\n\n{file}:{}{range}\n> {}\n{suggestion}",
    comment.start_line, comment.comment_text
  )
}

// Agent deep links.

/// `treq://agent/start`: start an agent on a workspace's branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDeepLinkRequest {
  pub repo: String,
  pub branch: String,
  pub prompt: String,
  /// `plan` or `acceptEdits`.
  pub mode: String,
  pub agent: String,
  pub request_id: String,
}

fn normalize_mode(value: &str) -> Option<&'static str> {
  match value {
    "plan" => Some("plan"),
    "acceptEdits" | "edit" => Some("acceptEdits"),
    _ => None,
  }
}

/// `parseAgentDeepLinkUrl`.
pub fn parse_agent_deep_link_url(url: &str) -> Option<AgentDeepLinkRequest> {
  let query = url.strip_prefix("treq://agent/start?")?;
  let mut values = HashMap::new();
  for pair in query.split('&') {
    let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
    let value = urlencoding::decode(&value.replace('+', " "))
      .ok()?
      .into_owned();
    values.entry(key.to_string()).or_insert(value);
  }
  let get = |key: &str| values.get(key).filter(|v| !v.is_empty()).cloned();
  Some(AgentDeepLinkRequest {
    repo: get("repo")?,
    branch: get("branch")?,
    prompt: get("prompt")?,
    mode: normalize_mode(&get("mode")?)?.to_string(),
    agent: agent_kind(get("agent").as_deref())?,
    request_id: get("request_id")?,
  })
}

/// What to do with a deep link request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeepLinkRoute {
  /// Already handled, or another window claimed it.
  Skip,
  /// Start it in this window.
  Start,
  /// Queued for a window of its repository, which the caller opens.
  OpenWindow,
}

/// The requests seen so far, across windows (`localStorage`'s
/// processed, claim and pending keys).
#[derive(Debug, Default)]
pub struct DeepLinks {
  processed: HashSet<String>,
  claimed: HashSet<String>,
  pending: HashMap<String, Vec<AgentDeepLinkRequest>>,
}

impl DeepLinks {
  pub fn is_processed(&self, request_id: &str) -> bool {
    self.processed.contains(request_id)
  }

  pub fn mark_processed(&mut self, request_id: &str) {
    self.processed.insert(request_id.to_string());
  }

  /// `tryClaimAgentRequest`: only the first claim wins.
  pub fn try_claim(&mut self, request_id: &str) -> bool {
    self.claimed.insert(request_id.to_string())
  }

  /// `queuePendingAgentRequest`, without duplicates.
  pub fn queue(&mut self, request: AgentDeepLinkRequest) {
    let queued = self.pending.entry(request.repo.clone()).or_default();
    if !queued.iter().any(|r| r.request_id == request.request_id) {
      queued.push(request);
    }
  }

  /// `popPendingAgentRequests`.
  pub fn pop(&mut self, repo: &str) -> Vec<AgentDeepLinkRequest> {
    self.pending.remove(repo).unwrap_or_default()
  }

  /// `processAgentDeepLinkRequests` for one request, in a window showing
  /// `repos` (its main repository, then supporting ones); `None` for no
  /// window.
  pub fn route(
    &mut self,
    request: &AgentDeepLinkRequest,
    repos: Option<&[String]>,
  ) -> DeepLinkRoute {
    if self.is_processed(&request.request_id) || !self.try_claim(&request.request_id) {
      return DeepLinkRoute::Skip;
    }
    if repos.is_some_and(|repos| repos.contains(&request.repo)) {
      return DeepLinkRoute::Start;
    }
    self.queue(request.clone());
    DeepLinkRoute::OpenWindow
  }
}

/// The app's deep link requests.
pub fn deep_links() -> &'static Mutex<DeepLinks> {
  static LINKS: OnceLock<Mutex<DeepLinks>> = OnceLock::new();
  LINKS.get_or_init(Default::default)
}

/// `findWorkspaceByBranch`.
pub fn find_workspace_by_branch<'a>(
  workspaces: &'a [crate::local_db::Workspace],
  branch: &str,
) -> Option<&'a crate::local_db::Workspace> {
  workspaces.iter().find(|w| w.branch_name == branch)
}

// `treq send` attachments.

/// `TreqSendAsset`: a file `treq send` handed a terminal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SendAsset {
  pub id: String,
  pub repo: String,
  pub pty_session_id: Option<String>,
  /// `image`, `text` or `browser`.
  pub media_type: String,
  pub path: String,
  pub title: String,
}

/// `parseTreqSendPayload`.
pub fn send_asset(request: &crate::send_dispatch::SendDispatchRequest) -> Option<SendAsset> {
  if request.request_id.is_empty() || request.path.is_empty() {
    return None;
  }
  let title = request
    .title
    .as_deref()
    .map(str::trim)
    .filter(|t| !t.is_empty())
    .map(str::to_string)
    .or_else(|| {
      request
        .path
        .rsplit(['/', '\\'])
        .next()
        .filter(|n| !n.is_empty())
        .map(str::to_string)
    })
    .unwrap_or_else(|| request.path.clone());
  let media_type = match request.media_type.as_str() {
    "image" | "browser" => request.media_type.clone(),
    _ => "text".into(),
  };
  Some(SendAsset {
    id: request.request_id.clone(),
    repo: request.repo.clone(),
    pty_session_id: request
      .pty_session_id
      .clone()
      .filter(|id| !id.trim().is_empty()),
    media_type,
    path: request.path.clone(),
    title,
  })
}

fn send_assets_store() -> &'static Mutex<Vec<SendAsset>> {
  static ASSETS: OnceLock<Mutex<Vec<SendAsset>>> = OnceLock::new();
  ASSETS.get_or_init(Default::default)
}

/// Keeps `asset` until dismissed; false when it was already there.
pub fn add_send_asset(asset: SendAsset) -> bool {
  let mut assets = send_assets_store().lock_or_recover();
  if assets.iter().any(|a| a.id == asset.id) {
    return false;
  }
  assets.push(asset);
  true
}

pub fn dismiss_send_asset(id: &str) {
  send_assets_store().lock_or_recover().retain(|a| a.id != id);
}

/// The attachments not dismissed yet, oldest first.
pub fn send_assets() -> Vec<SendAsset> {
  send_assets_store().lock_or_recover().clone()
}

#[cfg(test)]
mod tests {
  use std::cell::RefCell;

  use super::*;

  #[derive(Default)]
  struct FakeStore {
    calls: RefCell<Vec<String>>,
    settings: HashMap<String, String>,
    sessions: usize,
    fail_issue: Option<String>,
  }

  impl LaunchStore for FakeStore {
    fn create_session(&self, repo: &str, ws: Option<i64>, name: &str) -> Result<i64, String> {
      self
        .calls
        .borrow_mut()
        .push(format!("session {repo} {ws:?} {name}"));
      Ok(7)
    }
    fn session_count(&self, _: &str, _: Option<i64>) -> usize {
      self.sessions
    }
    fn repo_setting(&self, _: &str, key: &str) -> Option<String> {
      self.settings.get(&format!("repo.{key}")).cloned()
    }
    fn app_setting(&self, key: &str) -> Option<String> {
      self.settings.get(key).cloned()
    }
    fn set_repo_setting(&self, _: &str, key: &str, value: &str) -> Result<(), String> {
      self.calls.borrow_mut().push(format!("set {key}={value}"));
      Ok(())
    }
    fn add_prompt_history(
      &self,
      _: &str,
      ws: Option<i64>,
      session: i64,
      prompt: &str,
      agent: Option<&str>,
    ) -> Result<(), String> {
      self
        .calls
        .borrow_mut()
        .push(format!("history {ws:?} {session} {prompt} {agent:?}"));
      Ok(())
    }
    fn open_issue_workspace(
      &self,
      repo: &str,
      _: IssueSource,
      id: &str,
      _: &str,
      _: &str,
    ) -> Result<(i64, bool), String> {
      self.calls.borrow_mut().push(format!("issue {repo} {id}"));
      if self.fail_issue.as_deref() == Some(id) {
        return Err("jj failed".into());
      }
      let n = self.calls.borrow().len() as i64;
      Ok((n, true))
    }
  }

  fn github_issue() -> IssueAttachment {
    IssueAttachment {
      source: IssueSource::Github,
      id: "42".into(),
      key: "#42".into(),
      url: "https://github.com/acme/treq/issues/42".into(),
      title: "Fix the login bug".into(),
      include_sub_items: false,
      sub_item_ids: vec![],
    }
  }

  fn linear_issue() -> IssueAttachment {
    IssueAttachment {
      source: IssueSource::Linear,
      id: "parent".into(),
      key: "ENG-1".into(),
      url: "https://linear.app/t/issue/ENG-1".into(),
      title: "Parent".into(),
      include_sub_items: true,
      sub_item_ids: vec!["child-a".into(), "child-b".into()],
    }
  }

  #[test]
  fn appends_the_issue_reference_to_a_typed_prompt() {
    assert_eq!(
      format_prompt_with_issue("fix auth redirect", &github_issue()),
      "fix auth redirect\n\nGitHub issue #42: https://github.com/acme/treq/issues/42"
    );
  }

  #[test]
  fn builds_a_default_prompt_from_the_issue() {
    assert_eq!(
      format_prompt_with_issue("  ", &github_issue()),
      "Address GitHub issue #42: Fix the login bug\n\nhttps://github.com/acme/treq/issues/42"
    );
    let untitled = IssueAttachment {
      title: String::new(),
      ..github_issue()
    };
    assert_eq!(
      format_prompt_with_issue("", &untitled),
      "Address GitHub issue #42\n\nhttps://github.com/acme/treq/issues/42"
    );
  }

  #[test]
  fn names_each_source_with_its_own_label() {
    let issue = IssueAttachment {
      source: IssueSource::Linear,
      key: "ENG-101".into(),
      url: "https://example.test/x".into(),
      ..github_issue()
    };
    assert_eq!(
      format_prompt_with_issue("do it", &issue),
      "do it\n\nLinear issue ENG-101: https://example.test/x"
    );
  }

  #[test]
  fn issue_attachments_read_the_route_shape() {
    let issue: IssueAttachment = serde_json::from_value(serde_json::json!({
      "source": "linear", "id": "issue-1", "key": "ENG-101",
      "url": "u", "title": "t", "includeSubItems": true, "subItemIds": ["child-1"],
    }))
    .unwrap();
    assert_eq!(issue.source, IssueSource::Linear);
    assert_eq!(issue.sub_item_ids, vec!["child-1".to_string()]);
  }

  #[test]
  fn opens_the_parent_then_each_sub_issue() {
    let store = FakeStore::default();
    let result = open_or_create_issue_workspace(&store, "/repo", &linear_issue()).unwrap();
    assert_eq!(
      *store.calls.borrow(),
      [
        "issue /repo parent",
        "issue /repo child-a",
        "issue /repo child-b"
      ]
    );
    assert_eq!(result.workspace_id, 1);
    assert_eq!(result.sub_item_results.len(), 2);
    assert!(result.sub_item_failures.is_empty());
  }

  #[test]
  fn keeps_going_after_a_sub_issue_fails() {
    let store = FakeStore {
      fail_issue: Some("child-a".into()),
      ..Default::default()
    };
    let result = open_or_create_issue_workspace(&store, "/repo", &linear_issue()).unwrap();
    assert_eq!(store.calls.borrow().len(), 3);
    assert_eq!(result.sub_item_results.len(), 1);
    assert_eq!(
      result.sub_item_failures,
      vec![("child-a".to_string(), "jj failed".to_string())]
    );
    let toasts = sub_item_toasts(&result);
    assert_eq!(toasts[0].title, "Sub-issue workspaces ready");
    assert_eq!(toasts[0].description, "Created 1.");
    assert_eq!(toasts[1].title, "1 sub-issue workspace failed");
    assert!(toasts[1].warning);
  }

  #[test]
  fn skips_sub_issues_unless_asked() {
    let store = FakeStore::default();
    let issue = IssueAttachment {
      include_sub_items: false,
      ..linear_issue()
    };
    open_or_create_issue_workspace(&store, "/repo", &issue).unwrap();
    assert_eq!(*store.calls.borrow(), ["issue /repo parent"]);
  }

  #[test]
  fn kickoffs_never_overlap() {
    struct Slow(
      std::sync::atomic::AtomicUsize,
      std::sync::atomic::AtomicUsize,
    );
    impl LaunchStore for Slow {
      fn create_session(&self, _: &str, _: Option<i64>, _: &str) -> Result<i64, String> {
        Ok(1)
      }
      fn session_count(&self, _: &str, _: Option<i64>) -> usize {
        0
      }
      fn repo_setting(&self, _: &str, _: &str) -> Option<String> {
        None
      }
      fn app_setting(&self, _: &str) -> Option<String> {
        None
      }
      fn set_repo_setting(&self, _: &str, _: &str, _: &str) -> Result<(), String> {
        Ok(())
      }
      fn add_prompt_history(
        &self,
        _: &str,
        _: Option<i64>,
        _: i64,
        _: &str,
        _: Option<&str>,
      ) -> Result<(), String> {
        Ok(())
      }
      fn open_issue_workspace(
        &self,
        _: &str,
        _: IssueSource,
        _: &str,
        _: &str,
        _: &str,
      ) -> Result<(i64, bool), String> {
        use std::sync::atomic::Ordering::SeqCst;
        let now = self.0.fetch_add(1, SeqCst) + 1;
        self.1.fetch_max(now, SeqCst);
        std::thread::sleep(std::time::Duration::from_millis(5));
        self.0.fetch_sub(1, SeqCst);
        Ok((1, true))
      }
    }
    let store = std::sync::Arc::new(Slow(Default::default(), Default::default()));
    let threads: Vec<_> = (0..3)
      .map(|_| {
        let store = store.clone();
        std::thread::spawn(move || {
          open_or_create_issue_workspace(store.as_ref(), "/repo", &linear_issue()).unwrap();
        })
      })
      .collect();
    for t in threads {
      t.join().unwrap();
    }
    assert_eq!(store.1.load(std::sync::atomic::Ordering::SeqCst), 1);
  }

  #[test]
  fn start_agent_task_creates_the_session_with_its_start() {
    let store = FakeStore::default();
    let task = TaskRequest {
      repo_path: "/repo".into(),
      workspace_id: Some(3),
      text: "  fix it  ".into(),
      agent: "codex".into(),
      mode: "plan".into(),
      save_as_repo_default: true,
      ..Default::default()
    };
    let (start, toasts) = start_agent_task(&store, &task).unwrap().unwrap();
    assert!(toasts.is_empty());
    assert_eq!(
      *store.calls.borrow(),
      ["set default_agent=codex", "session /repo Some(3) fix it"]
    );
    assert_eq!(start.pending_prompt.as_deref(), Some("fix it"));
    assert_eq!(start.permission_mode.as_deref(), Some("plan"));
    assert_eq!(start.agent.as_deref(), Some("codex"));
    assert_eq!(start.workspace_id, Some(3));
  }

  #[test]
  fn start_agent_task_runs_in_the_issue_workspace() {
    let store = FakeStore::default();
    let task = TaskRequest {
      repo_path: "/repo".into(),
      workspace_id: Some(3),
      issue: Some(github_issue()),
      agent: "claude".into(),
      mode: "acceptEdits".into(),
      ..Default::default()
    };
    let (start, _) = start_agent_task(&store, &task).unwrap().unwrap();
    assert_eq!(start.workspace_id, Some(1));
    assert_eq!(
      start.session_name,
      "Address GitHub issue #42: Fix the login bug\n\nht..."
    );
  }

  #[test]
  fn start_agent_task_sends_nothing_for_an_empty_prompt() {
    let store = FakeStore::default();
    let task = TaskRequest {
      text: "   ".into(),
      ..Default::default()
    };
    assert_eq!(start_agent_task(&store, &task).unwrap(), None);
    assert!(store.calls.borrow().is_empty());
  }

  #[test]
  fn session_names_stop_at_fifty_characters() {
    assert_eq!(session_name("short"), "short");
    let long = "x".repeat(60);
    assert_eq!(session_name(&long), format!("{}...", "x".repeat(47)));
  }

  #[test]
  fn launch_names_unnamed_sessions_after_the_agent() {
    let store = FakeStore {
      sessions: 2,
      ..Default::default()
    };
    let start = launch_session(
      &store,
      &LaunchRequest {
        repo_path: "/repo".into(),
        agent: Some("codex".into()),
        ..Default::default()
      },
    )
    .unwrap();
    assert_eq!(start.session_name, "Codex 3");
    record_prompt(&store, &start).unwrap();
    // No prompt: nothing recorded.
    assert_eq!(store.calls.borrow().len(), 1);
  }

  #[test]
  fn records_the_prompt_in_the_history() {
    let store = FakeStore::default();
    let start = SessionStart {
      session_id: 4,
      workspace_id: Some(2),
      pending_prompt: Some("go".into()),
      agent: Some("claude".into()),
      ..Default::default()
    };
    record_prompt(&store, &start).unwrap();
    assert_eq!(
      *store.calls.borrow(),
      ["history Some(2) 4 go Some(\"claude\")"]
    );
  }

  #[test]
  fn resolves_the_agent_from_override_then_repo_then_app() {
    let mut store = FakeStore::default();
    assert_eq!(resolve_agent(&store, "/r", None), None);
    store
      .settings
      .insert("default_agent".into(), "cursor".into());
    assert_eq!(resolve_agent(&store, "/r", None).as_deref(), Some("cursor"));
    store
      .settings
      .insert("repo.default_agent".into(), "codex".into());
    assert_eq!(resolve_agent(&store, "/r", None).as_deref(), Some("codex"));
    assert_eq!(
      resolve_agent(&store, "/r", Some("copilot")).as_deref(),
      Some("copilot")
    );
    assert_eq!(resolve_agent(&store, "/r", Some("bogus")), None);
  }

  #[test]
  fn pending_sessions_are_kept_per_window() {
    let pending = PendingSession {
      pending_prompt: Some("hi".into()),
      ..Default::default()
    };
    remember_pending("w-test-1", 5, pending.clone());
    remember_pending("w-test-1", 6, PendingSession::default());
    assert_eq!(pending_session("w-test-1", 5), Some(pending));
    assert_eq!(pending_session("w-test-1", 6), None);
    assert_eq!(pending_session("w-test-2", 5), None);
    forget_window("w-test-1");
    assert_eq!(pending_session("w-test-1", 5), None);
  }

  #[test]
  fn main_repository_pane_ids_stay_as_they_are() {
    assert_eq!(pane_session_id("/repo", "/repo", 3), 3);
    let other = pane_session_id("/other", "/repo", 3);
    assert!(other > 1_000_000);
    assert_eq!(other % 1_000_000, 3);
    assert_ne!(other, pane_session_id("/third", "/repo", 3));
  }

  #[test]
  fn pty_ids_carry_a_base36_repo_hash() {
    assert_eq!(agent_pty_id("", 4), "session-0-4");
    assert_eq!(agent_pty_id("a", 4), "session-2p-4");
    assert_eq!(base36(0), "0");
    assert_eq!(base36(35), "z");
    assert_eq!(base36(36), "10");
    assert_eq!(
      pane_session_id("/api", "/repo", 3),
      (repo_hash("/api") as i64 + 1) * 1_000_000 + 3
    );
  }

  #[test]
  fn pty_ids_differ_by_repository_and_stay_event_safe() {
    assert_ne!(agent_pty_id("/a/repo", 1), agent_pty_id("/b/repo", 1));
    assert_eq!(agent_pty_id("/a/repo", 1), agent_pty_id("/a/repo", 1));
    let id = agent_pty_id("/Users/me/My Repo (1)", 12);
    assert!(
      id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
      "{id}"
    );
  }

  #[test]
  fn session_starts_read_like_session_creation_info() {
    let start = SessionStart {
      session_id: 1,
      session_name: "n".into(),
      workspace_id: Some(2),
      repo_path: "/r".into(),
      pending_prompt: Some("p".into()),
      ..Default::default()
    };
    let value = serde_json::to_value(&start).unwrap();
    assert_eq!(value["sessionId"], 1);
    assert_eq!(value["workspaceId"], 2);
    assert_eq!(value["pendingPrompt"], "p");
    assert_eq!(
      serde_json::from_value::<SessionStart>(value).unwrap(),
      start
    );
  }

  #[test]
  fn auto_review_summary_names_the_operation_and_branch() {
    assert!(auto_review_summary("on-commit", Some("feature")).contains("the commit just created"));
    assert!(auto_review_summary("on-rebase", Some("feature")).contains("the rebase just applied"));
    assert!(auto_review_summary("on-pull", Some("feature")).contains("pulled from the remote"));
    assert!(
      auto_review_summary("on-commit", Some("feature")).contains("the feature workspace diff")
    );
    assert!(auto_review_summary("on-commit", None).contains("the current workspace diff"));
  }

  #[test]
  fn workspace_review_uses_the_repository_prompt_and_agent() {
    let mut store = FakeStore::default();
    let (prompt, agent) = prepare_workspace_review(&store, "/r", 9, "the diff");
    assert!(prompt.contains("--target-type workspace_diff"));
    assert!(prompt.contains("--target-id 9"));
    assert_eq!(agent, None);
    store.settings.insert(
      "repo.review_prompt".into(),
      "Review {workspace_id}: {diff_summary}".into(),
    );
    store
      .settings
      .insert("repo.review_agent".into(), "codex".into());
    let (prompt, agent) = prepare_workspace_review(&store, "/r", 9, "the diff");
    assert_eq!(prompt, "Review 9: the diff");
    assert_eq!(agent.as_deref(), Some("codex"));
  }

  #[test]
  fn review_comment_prompts_cite_the_lines_and_suggestion() {
    let comment = crate::local_db::AgentReviewComment {
      id: "c".into(),
      repo_path: "/r".into(),
      target_type: "workspace_diff".into(),
      target_id: "1".into(),
      file_path: "/ws/src/a.rs".into(),
      hunk_id: None,
      start_line: 3,
      end_line: 5,
      side: None,
      comment_text: "off by one".into(),
      suggested_replacement: Some("x + 1".into()),
      status: "open".into(),
      source: "local-agent".into(),
      created_at: String::new(),
      resolved_at: None,
      quoted_text: None,
      line_hash: None,
    };
    assert_eq!(
      review_comment_prompt(&comment, "/ws"),
      "Please address this review comment.\n\nsrc/a.rs:3-5\n> off by one\n\nSuggested change:\n```\nx + 1\n```\n"
    );
  }

  #[test]
  fn parses_an_agent_deep_link() {
    let parsed = parse_agent_deep_link_url(
      "treq://agent/start?repo=%2Ftmp%2Frepo&branch=feat%2Fone&prompt=hello&mode=edit&agent=codex&request_id=req-1",
    );
    assert_eq!(
      parsed,
      Some(AgentDeepLinkRequest {
        repo: "/tmp/repo".into(),
        branch: "feat/one".into(),
        prompt: "hello".into(),
        mode: "acceptEdits".into(),
        agent: "codex".into(),
        request_id: "req-1".into(),
      })
    );
  }

  #[test]
  fn rejects_malformed_deep_links() {
    assert_eq!(
      parse_agent_deep_link_url("treq://auth/callback?token=abc"),
      None
    );
    assert_eq!(
      parse_agent_deep_link_url(
        "treq://agent/start?repo=a&branch=b&prompt=c&mode=bad&agent=codex&request_id=1"
      ),
      None
    );
  }

  fn request(repo: &str, id: &str) -> AgentDeepLinkRequest {
    AgentDeepLinkRequest {
      repo: repo.into(),
      branch: "feat/x".into(),
      prompt: "do it".into(),
      mode: "acceptEdits".into(),
      agent: "claude".into(),
      request_id: id.into(),
    }
  }

  #[test]
  fn deep_links_find_their_workspace_by_exact_branch_only() {
    let workspace = |id: i64, branch: &str| -> crate::local_db::Workspace {
      serde_json::from_value(serde_json::json!({
        "id": id, "repo_path": "/repo", "workspace_name": branch, "workspace_path": branch,
        "branch_name": branch, "created_at": "", "refreshed_at": null, "metadata": null,
        "target_branch": null, "title": branch, "description": null, "moved_files": null,
        "not_on_remote": false, "sparse_patterns": null,
      }))
      .unwrap()
    };
    let workspaces = vec![workspace(1, "feat/agent-link"), workspace(2, "feat/other")];
    assert_eq!(
      find_workspace_by_branch(&workspaces, "feat/agent-link").map(|w| w.id),
      Some(1)
    );
    assert!(find_workspace_by_branch(&workspaces, "missing").is_none());
    assert!(find_workspace_by_branch(&workspaces, "feat").is_none());
  }

  #[test]
  fn deep_links_track_processed_ids_and_first_claims() {
    let mut links = DeepLinks::default();
    assert!(!links.is_processed("p"));
    links.mark_processed("p");
    assert!(links.is_processed("p"));
    assert!(links.try_claim("c"));
    assert!(!links.try_claim("c"));
  }

  #[test]
  fn deep_links_queue_per_repo_without_duplicates() {
    let mut links = DeepLinks::default();
    links.queue(request("/repo", "q"));
    links.queue(request("/repo", "q"));
    assert_eq!(links.pop("/repo"), vec![request("/repo", "q")]);
    assert!(links.pop("/repo").is_empty());
  }

  #[test]
  fn deep_links_start_in_a_window_showing_the_repository() {
    let mut links = DeepLinks::default();
    let repos = vec!["/repos/app".to_string(), "/repos/api".to_string()];
    assert_eq!(
      links.route(&request("/repos/api", "sup-1"), Some(&repos)),
      DeepLinkRoute::Start
    );
    // Claimed once: a second delivery is skipped.
    assert_eq!(
      links.route(&request("/repos/api", "sup-1"), Some(&repos)),
      DeepLinkRoute::Skip
    );
  }

  #[test]
  fn deep_links_for_other_repositories_open_a_window() {
    let mut links = DeepLinks::default();
    let repos = vec!["/repos/app".to_string()];
    assert_eq!(
      links.route(&request("/repos/other", "o-1"), Some(&repos)),
      DeepLinkRoute::OpenWindow
    );
    assert_eq!(links.pop("/repos/other").len(), 1);
    links.mark_processed("o-2");
    assert_eq!(
      links.route(&request("/repos/other", "o-2"), None),
      DeepLinkRoute::Skip
    );
  }

  #[test]
  fn send_assets_read_the_dispatch_request() {
    let mut request =
      crate::send_dispatch::SendDispatchRequest::new("r1", "/repo", "png", "/tmp/a/shot.png");
    request.pty_session_id = Some("  ".into());
    let asset = send_asset(&request).unwrap();
    assert_eq!(asset.title, "shot.png");
    assert_eq!(asset.media_type, "text");
    assert_eq!(asset.pty_session_id, None);
    request.title = Some(" Shot ".into());
    request.media_type = "image".into();
    let asset = send_asset(&request).unwrap();
    assert_eq!(
      (asset.title.as_str(), asset.media_type.as_str()),
      ("Shot", "image")
    );
    assert!(add_send_asset(asset.clone()));
    assert!(!add_send_asset(asset));
    assert!(send_assets().iter().any(|a| a.id == "r1"));
    dismiss_send_asset("r1");
    assert!(!send_assets().iter().any(|a| a.id == "r1"));
  }
}
