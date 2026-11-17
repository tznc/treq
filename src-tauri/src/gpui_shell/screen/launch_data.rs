//! What the screens ask of the backend to start agent sessions
//! (`core::agent_launch`). The backend keeps each new session's start data
//! for the window's terminal pane and tells React which session to show.

pub use crate::core::agent_launch::{
  LaunchRequest, PendingSession, SendAsset, SessionStart, TaskRequest,
};

/// Starts agent sessions for a window's screens; faked in tests.
pub trait LaunchHost: Send + Sync {
  /// The prompt dialog's Edit or Plan (`startAgentTask`); false when there
  /// was nothing to send.
  fn start_agent_task(&self, task: &TaskRequest) -> Result<bool, String>;
  /// Creates the session `request` asks for and starts it; `select` also
  /// shows its workspace.
  fn launch_session(&self, request: &LaunchRequest, select: bool) -> Result<SessionStart, String>;
  /// A session a screen created itself (`handleSessionCreated`): keeps its
  /// start data, records its prompt and tells React to show it.
  fn session_started(&self, start: &SessionStart, select: bool);
  /// `resolveDefaultAgent`.
  fn resolve_agent(&self, repo_path: &str, agent: Option<&str>) -> Option<String>;
  /// What the terminal starts pane session `pane_id` with.
  fn pending_session(&self, pane_id: i64) -> Option<PendingSession>;
  /// `treq send` attachments not dismissed yet.
  fn send_assets(&self) -> Vec<SendAsset>;
  fn dismiss_send_asset(&self, id: &str);
  /// Keeps a review sent to an agent in the review history; `scope` is
  /// `changes` or `files`.
  fn record_review(
    &self,
    repo_path: &str,
    workspace_id: Option<i64>,
    scope: &str,
    markdown: &str,
    mode: &str,
  );
}

/// `onSessionCreated` for a session a screen created: `session` is its
/// `SessionCreationInfo`. Returns it, for the caller's own use.
pub fn started(host: &dyn LaunchHost, session: serde_json::Value) -> serde_json::Value {
  match serde_json::from_value::<SessionStart>(session.clone()) {
    Ok(start) => host.session_started(&start, false),
    Err(err) => log::warn!("failed to read a created session: {err}"),
  }
  session
}
