use crate::result_ext::ResultExt;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::jj;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SshHost {
  pub alias: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteReadinessCheck {
  pub name: String,
  pub available: bool,
  pub detail: String,
  /// Distinct structured error code for this check, when it failed for a
  /// reason more specific than "unavailable" (PRD "Resource quotas": quota
  /// failures "must be a distinct, structured readiness or mutation error
  /// so the UI can explain the quota rather than surfacing a generic
  /// filesystem or provider failure"). `None` for ordinary binary-presence
  /// checks; `Some("disk_quota_exceeded")` when the base disk quota check
  /// fails.
  #[serde(skip_serializing_if = "Option::is_none", default)]
  pub code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteReadiness {
  pub host: String,
  pub connected: bool,
  pub checks: Vec<RemoteReadinessCheck>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteRepoProbe {
  pub host: String,
  pub path: String,
  pub exists: bool,
  pub is_repo: bool,
  pub needs_clone: bool,
}

/// The directories in one directory on a machine ([`TreqCommandRequest::ListDirectories`]).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectoryListing {
  /// The directory, absolute (`~` resolved).
  pub path: String,
  /// Its parent; `None` at the root.
  pub parent: Option<String>,
  /// It holds a Git or jj repository.
  pub is_repo: bool,
  /// Its subdirectories, by name.
  pub entries: Vec<DirectoryEntry>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectoryEntry {
  pub name: String,
  pub path: String,
  /// The name starts with a dot.
  pub hidden: bool,
  pub is_repo: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RepositoryLocation {
  Local { path: String },
  Ssh { host: String, path: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryDescriptor {
  pub id: String,
  pub location: RepositoryLocation,
  pub display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryInspection {
  pub root: String,
  pub repository_type: String,
  pub current_branch: Option<String>,
  pub default_branch: String,
  pub current_change_id: String,
  pub current_commit_id: String,
  pub descriptor: RepositoryDescriptor,
}

/// Cheap, pollable change marker for a workspace's JJ operation log (PRD
/// "Change propagation across concurrent clients"). A client stores the
/// last `operation_id` it observed and compares it against the latest
/// value; a mismatch means repository state moved on the VM for a reason
/// other than the client's own in-flight mutation, and the client should
/// refresh status/changes/commits/conflicts rather than merge or reconcile
/// anything. This is stale-state notification only, never conflict
/// resolution.
///
/// Reading the marker snapshots the workspace's working copy first, so file
/// edits made outside jj (an editor, a terminal, an agent) also move it. An
/// idle workspace keeps the same marker and gains no new operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceChangeMarker {
  pub operation_id: String,
  /// Change id of the workspace's working-copy commit when the marker was
  /// read. A `CreateCommit` records it before running so a post-reconnect
  /// check can tell "this exact working copy was committed" apart from "some
  /// other commit happens to carry the same message".
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub working_copy_change_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CliErrorBody {
  pub error: CliErrorDetail,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CliErrorDetail {
  pub code: String,
  pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum TreqCommandRequest {
  InspectRepository {
    repo: String,
  },
  RepositoryStatus {
    repo: String,
  },
  ListBranches {
    repo: String,
  },
  ListWorkspaces {
    repo: String,
  },
  /// The workspaces with their sidebar status (conflicts, uncommitted
  /// changes, commits ahead, running terminals). See
  /// [`crate::core::workspaces::WorkspaceSummary`].
  WorkspaceSummaries {
    repo: String,
  },
  /// What each line of `text` would open or make, without doing it. See
  /// [`crate::core::new_workspaces::plan`].
  PlanNewWorkspaces {
    repo: String,
    text: String,
    stack_on: Option<String>,
  },
  InspectWorkspace {
    repo: String,
    workspace: String,
  },
  ListChanges {
    repo: String,
    workspace: Option<String>,
  },
  DiffFile {
    repo: String,
    workspace: Option<String>,
    path: String,
  },
  /// `DiffFile` for several files at once, from the trees `token` (a
  /// `working_copy_token` from `WorkspaceDiff`) names when given, else from
  /// a snapshot. Answers a `WorkspaceFileHunksBatch`; a host older than it
  /// fails, so callers fall back to `DiffFile`.
  DiffFiles {
    repo: String,
    workspace: Option<String>,
    paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    token: Option<String>,
  },
  ReadFile {
    repo: String,
    workspace: Option<String>,
    path: String,
    revision: FileRevision,
    start_line: Option<usize>,
    end_line: Option<usize>,
  },
  ListCommits {
    repo: String,
    workspace: Option<String>,
  },
  ListConflicts {
    repo: String,
    workspace: Option<String>,
  },
  /// Full review diff for a workspace against its target branch: committed
  /// files, uncommitted files, conflicts, and hunks, matching the local
  /// `get_workspace_diff` Tauri command.
  WorkspaceDiff {
    repo: String,
    workspace: String,
  },
  /// Diff of a single commit (by change or commit id).
  CommitDiff {
    repo: String,
    workspace: Option<String>,
    revision: String,
  },
  /// Diff of one file inside a single commit.
  CommitFileDiff {
    repo: String,
    workspace: Option<String>,
    revision: String,
    path: String,
  },
  /// One directory of the workspace's files (Git-ignored entries left out),
  /// directories first; `path` is relative to the workspace, empty for its
  /// root. See [`RemoteFileEntry`].
  ListFiles {
    repo: String,
    workspace: Option<String>,
    #[serde(default)]
    path: String,
  },
  /// Fuzzy file-name search over the workspace file index.
  SearchFiles {
    repo: String,
    workspace: Option<String>,
    query: String,
    #[serde(default)]
    limit: Option<usize>,
  },
  /// Returns the workspace's current JJ operation id as a lightweight,
  /// pollable change marker so a client can detect that another client (or
  /// process) moved VM-side repository state and refresh. See
  /// [`WorkspaceChangeMarker`].
  WorkspaceChangeMarker {
    repo: String,
    workspace: Option<String>,
  },
  /// Totals for every repository under `root` on the machine: repository
  /// and workspace counts plus the bytes they occupy (the same figure the
  /// disk quota is enforced against). See [`MachineUsageReport`].
  MachineUsage {
    root: String,
  },
  /// Which of Git, jj, tmux and the agent CLIs the machine has
  /// ([`crate::core::prerequisites::HOST_BINARIES`]); `root` is unused but
  /// the CLI requires `--repo`.
  MachineTools {
    root: String,
  },
  /// The directories in `path` on the machine (`~` is its home), for
  /// browsing to a repository. See [`DirectoryListing`].
  ListDirectories {
    path: String,
  },
  /// What the host can run: the treq version, a terminal multiplexer for
  /// persistent sessions, and which agent CLIs are installed. `root` is the
  /// folder repositories are opened from (`~` for the home folder). See
  /// [`RemoteHostCheck`].
  HostCheck {
    root: String,
  },
  // -- Phase 5: probe/clone/init as typed commands ---------------------------
  ProbeRepo {
    repo: String,
  },
  CloneRepo {
    repo_url: String,
    destination: String,
    idempotency_key: String,
  },
  InitRepo {
    repo: String,
    idempotency_key: String,
  },
  // -- Phase 5: workspace mutations -------------------------------------------
  /// Opens or makes a workspace for each line of `text`, stacked as the
  /// lines are indented. See [`crate::core::new_workspaces::run`].
  NewWorkspaces {
    repo: String,
    text: String,
    stack_on: Option<String>,
    idempotency_key: String,
  },
  CreateWorkspace {
    repo: String,
    branch_name: String,
    source_branch: Option<String>,
    /// Same JSON the local `create_workspace` Tauri command accepts (title,
    /// description, moved files, sparse patterns, symlinked dirs).
    #[serde(default)]
    metadata: Option<String>,
    idempotency_key: String,
  },
  RenameWorkspace {
    repo: String,
    workspace: String,
    new_name: String,
    idempotency_key: String,
  },
  UpdateWorkspace {
    repo: String,
    workspace: String,
    target_branch: Option<String>,
    #[serde(default)]
    title: Option<String>,
    description: Option<String>,
  },
  DeleteWorkspace {
    repo: String,
    workspace: String,
  },
  /// Forgets the workspace and removes its directory but keeps its record,
  /// like the local `archive_workspace` command.
  ArchiveWorkspace {
    repo: String,
    workspace: String,
  },
  MoveWorkspaceChanges {
    repo: String,
    workspace: String,
    destination: String,
    #[serde(default)]
    files: Vec<String>,
    #[serde(default)]
    hunks: Vec<crate::core::workspaces::HunkSpec>,
    commits: Vec<String>,
    idempotency_key: String,
  },
  RebaseWorkspace {
    repo: String,
    workspace: String,
    target_branch: String,
    idempotency_key: String,
  },
  /// Moves the repository's own working copy onto `bookmark`. Switching to
  /// the current branch again is a no-op, so it takes no idempotency key.
  SwitchRepoBranch {
    repo: String,
    bookmark: String,
  },
  // -- Phase 5: file mutations -------------------------------------------------
  RestoreFile {
    repo: String,
    workspace: Option<String>,
    path: String,
  },
  PatchFile {
    repo: String,
    workspace: Option<String>,
    path: String,
    patch_base64: String,
    idempotency_key: String,
  },
  /// Records the working copy so a later discard can be undone. Returns the
  /// snapshot id `RestoreSnapshot` takes.
  SnapshotWorkingCopy {
    repo: String,
    workspace: Option<String>,
  },
  RestoreAll {
    repo: String,
    workspace: Option<String>,
  },
  /// Undoes a discard by restoring the working copy to a snapshot.
  RestoreSnapshot {
    repo: String,
    workspace: Option<String>,
    snapshot_id: String,
  },
  /// Moves the working copy's changes into a new stash entry, stored in the
  /// repository's own `.treq/local.db` like the local command's.
  StashWorkspaceChanges {
    repo: String,
    workspace: Option<String>,
    idempotency_key: String,
  },
  // -- Phase 5: commit mutations ------------------------------------------------
  CreateCommit {
    repo: String,
    workspace: Option<String>,
    message: String,
    /// Working-copy change id observed before the commit ran. It is the
    /// marker the verify-before-retry check uses: a commit rewrites the
    /// working copy in place (keeping its change id) and starts a fresh one on
    /// top, so "this change id is no longer the working copy and now carries
    /// the message" means this exact commit landed. `None` makes that check
    /// report ambiguity instead of guessing from the message alone.
    #[serde(default)]
    base_change_id: Option<String>,
    idempotency_key: String,
  },
  DescribeCommit {
    repo: String,
    workspace: String,
    commit: String,
    message: String,
  },
  /// Non-interactive split: the caller supplies the files and/or hunks to
  /// place in the first commit. No interactive hunk selector runs on the
  /// exec channel. Only the working copy (`@`) can be split today, and only
  /// by whole files; other commits and hunk selections are rejected with a
  /// structured error rather than silently widened.
  SplitCommit {
    repo: String,
    workspace: String,
    commit: String,
    files: Vec<String>,
    hunks: Vec<crate::core::workspaces::HunkSpec>,
    /// Description for the new commit holding the selected files.
    #[serde(default)]
    message: String,
    idempotency_key: String,
  },
  MoveCommit {
    repo: String,
    workspace: String,
    commit: String,
    target_workspace: String,
    idempotency_key: String,
  },
  /// Abandons the workspace's tip commit, like the local `undo_commit`.
  UndoCommit {
    repo: String,
    workspace: String,
    commit: String,
  },
  AbandonCommit {
    repo: String,
    workspace: String,
    commit: String,
    idempotency_key: String,
  },
  /// Undoes `operation_id` (e.g. the id an abandon returned). The operation
  /// must still be the head, so a repeat fails instead of undoing twice.
  UndoOperation {
    repo: String,
    workspace: Option<String>,
    operation_id: String,
  },
  /// Adds a commit reversing `commit` on top of the workspace's tip. A
  /// repeat would add a second revert, so it needs an idempotency key.
  RevertCommit {
    repo: String,
    workspace: String,
    commit: String,
    idempotency_key: String,
  },
  // -- Phase 5: conflict mutations ----------------------------------------------
  ResolveConflict {
    repo: String,
    revision: String,
    sides: Vec<String>,
    idempotency_key: String,
  },
  // -- Phase 5: git operations ---------------------------------------------------
  GitFetch {
    repo: String,
  },
  /// Fetches and rebases a workspace onto its remote branch, like the local
  /// `pull_workspace_from_remote` command. Without a workspace it only
  /// fetches. Pulling again is a no-op, so it takes no idempotency key.
  PullWorkspace {
    repo: String,
    workspace: Option<String>,
  },
  GitBookmarkTrack {
    repo: String,
    bookmark: String,
    remote_name: String,
  },
  /// GitHub owner/name parsed from the repository's origin remote, the same
  /// read the local `get_git_remote_url` command does. `null` when absent.
  GitRemoteInfo {
    repo: String,
  },
  GitPush {
    repo: String,
    workspace: Option<String>,
    idempotency_key: String,
  },
  /// The workspace branch's GitHub pull request and its checks, read with
  /// the host's `gh`.
  WorkspacePullRequest {
    repo: String,
    workspace: String,
  },
  /// Pushes the workspace, then opens a pull request from its branch into
  /// its target branch with the host's `gh`.
  CreatePullRequest {
    repo: String,
    workspace: String,
    title: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    draft: bool,
    idempotency_key: String,
  },
  /// A GitHub read through the host's `gh`, against the repository's
  /// GitHub remote. See [`GithubQuery`].
  GithubRead {
    repo: String,
    query: GithubQuery,
  },
  /// A GitHub change through the host's `gh`, against the repository's
  /// GitHub remote. See [`GithubAction`].
  GithubWrite {
    repo: String,
    action: GithubAction,
    idempotency_key: String,
  },
  // -- Phase 5: agent lifecycle (VM-local supervisor) -----------------------------
  AgentStart {
    repo: String,
    workspace: String,
    agent: String,
    prompt: String,
    idempotency_key: String,
  },
  AgentInput {
    repo: String,
    workspace: String,
    input: String,
    idempotency_key: String,
  },
  AgentStatus {
    repo: String,
    workspace: String,
  },
  AgentStop {
    repo: String,
    workspace: String,
  },
  AgentLogs {
    repo: String,
    workspace: String,
  },
  // -- Phase 7: VM-local persistent PTY supervisor (`pty-remote`) -----------------
  /// Starts (or reuses, if already running) a persistent PTY session. Does not itself attach — the client attaches over a
  /// separate SSH PTY channel using `PtyAttachCommand`'s returned command
  /// line, matching how `core::remote_pty::build_launch_command` is used.
  PtyStart {
    /// Which program holds the session; omitted for the default daemon.
    #[serde(
      default,
      skip_serializing_if = "crate::core::pty_remote_supervisor::PtyBackend::is_default"
    )]
    backend: crate::core::pty_remote_supervisor::PtyBackend,
    repo: String,
    workspace: String,
    label: String,
    remote_dir: String,
    launch: crate::core::remote_pty::PtyLaunchSpec,
    cols: u16,
    rows: u16,
    idempotency_key: String,
  },
  /// Lists persistent PTY sessions, optionally scoped to `workspace`.
  PtyList {
    /// Which program holds the session; omitted for the default daemon.
    #[serde(
      default,
      skip_serializing_if = "crate::core::pty_remote_supervisor::PtyBackend::is_default"
    )]
    backend: crate::core::pty_remote_supervisor::PtyBackend,
    repo: String,
    workspace: Option<String>,
  },
  /// Ends every Treq session on the host under `backend`, whatever its
  /// repository, and for the daemon stops the daemon: a machine's "Reset
  /// terminals".
  /// What holds Treq's terminals on the host right now: the daemon (its
  /// pid, version and sessions) and Treq's sessions in tmux.
  PtyStatus,
  PtyReset {
    #[serde(
      default,
      skip_serializing_if = "crate::core::pty_remote_supervisor::PtyBackend::is_default"
    )]
    backend: crate::core::pty_remote_supervisor::PtyBackend,
  },
  /// Stops (kills) a persistent PTY session. Idempotent.
  PtyStop {
    /// Which program holds the session; omitted for the default daemon.
    #[serde(
      default,
      skip_serializing_if = "crate::core::pty_remote_supervisor::PtyBackend::is_default"
    )]
    backend: crate::core::pty_remote_supervisor::PtyBackend,
    repo: String,
    workspace: String,
    label: String,
  },
  /// Names a persistent PTY session; an empty title clears it. Idempotent.
  PtyRename {
    /// Which program holds the session; omitted for the default daemon.
    #[serde(
      default,
      skip_serializing_if = "crate::core::pty_remote_supervisor::PtyBackend::is_default"
    )]
    backend: crate::core::pty_remote_supervisor::PtyBackend,
    repo: String,
    workspace: String,
    label: String,
    title: String,
  },
  /// Returns the literal command line an SSH PTY channel should exec to
  /// attach (creating first if necessary) to a persistent session.
  PtyAttachCommand {
    /// Which program holds the session; omitted for the default daemon.
    #[serde(
      default,
      skip_serializing_if = "crate::core::pty_remote_supervisor::PtyBackend::is_default"
    )]
    backend: crate::core::pty_remote_supervisor::PtyBackend,
    repo: String,
    workspace: String,
    label: String,
    remote_dir: String,
    launch: crate::core::remote_pty::PtyLaunchSpec,
    cols: u16,
    rows: u16,
  },
  /// Pastes `text` into a running persistent session as one bracketed
  /// paste, then presses Enter when `submit`: how a review reaches an
  /// agent's terminal.
  PtyPaste {
    /// Which program holds the session; omitted for the default daemon.
    #[serde(
      default,
      skip_serializing_if = "crate::core::pty_remote_supervisor::PtyBackend::is_default"
    )]
    backend: crate::core::pty_remote_supervisor::PtyBackend,
    repo: String,
    workspace: String,
    label: String,
    text: String,
    submit: bool,
    idempotency_key: String,
  },
  /// `treq agent msg`: holds a message for an agent in a persistent
  /// session, in the host's mailbox for it (`core::agent_msg::mailbox`).
  /// Delivering the same message again replaces it.
  PtyMsgDeliver {
    repo: String,
    workspace: String,
    label: String,
    /// The message (`agent_msg::hub::Envelope`) as JSON.
    envelope: String,
  },
  /// What the agent in a persistent session did with its messages since
  /// event `since`; the events up to `since` are dropped.
  PtyMsgPoll {
    repo: String,
    workspace: String,
    label: String,
    since: u64,
  },
  /// The review a workspace's page left unsent on the host (empty if none).
  ReviewLoad {
    repo: String,
    workspace: String,
    scope: ReviewScope,
  },
  /// Keeps a workspace's unsent review on the host. Overwrites.
  ReviewSave {
    repo: String,
    workspace: String,
    scope: ReviewScope,
    review: crate::core::review::Stored,
  },
  /// Drops a workspace's unsent review, once it is sent or discarded.
  ReviewClear {
    repo: String,
    workspace: String,
    scope: ReviewScope,
  },
}

/// Which page a pending review belongs to: each keeps its own, as on
/// desktop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewScope {
  /// The Changes tab's diff.
  Changes,
  /// The file browser.
  Files,
}

impl ReviewScope {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::Changes => "changes",
      Self::Files => "files",
    }
  }

  pub fn parse(value: &str) -> Result<Self, String> {
    match value {
      "changes" => Ok(Self::Changes),
      "files" => Ok(Self::Files),
      other => Err(format!(
        "invalid_arguments: review scope must be changes or files, not '{other}'"
      )),
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileRevision {
  WorkingCopy,
  Parent,
}

// ---------------------------------------------------------------------------
// Retrying after network loss (PRD "Structured command protocol" >
// "Retrying after network loss")
// ---------------------------------------------------------------------------
//
// A network failure while a mutating command is in flight does not tell the
// client whether the command reached the VM, ran, or completed. This section
// classifies every `TreqCommandRequest` as a mutation or a pure read, and
// gives every mutation covered by "Remote mutation coverage" a verification
// recipe: a typed read command plus a check of its result that decides
// whether the mutation's effect is already observable in VM state. Pure
// reads are always safe to retry and are never classified as mutations.

impl TreqCommandRequest {
  /// True for every workspace, commit, conflict, Git, and agent-lifecycle
  /// mutation listed under "Remote mutation coverage"; false for every
  /// read/inspect command, which the PRD says is "always safe to retry".
  pub fn is_mutation(&self) -> bool {
    match self {
      // -- Pure reads: always safe to retry -----------------------------------
      Self::InspectRepository { .. }
      | Self::RepositoryStatus { .. }
      | Self::ListBranches { .. }
      | Self::ListWorkspaces { .. }
      | Self::WorkspaceSummaries { .. }
      | Self::PlanNewWorkspaces { .. }
      | Self::InspectWorkspace { .. }
      | Self::ListChanges { .. }
      | Self::DiffFile { .. }
      | Self::DiffFiles { .. }
      | Self::ReadFile { .. }
      | Self::ListCommits { .. }
      | Self::ListConflicts { .. }
      | Self::WorkspaceDiff { .. }
      | Self::CommitDiff { .. }
      | Self::CommitFileDiff { .. }
      | Self::SearchFiles { .. }
      | Self::ListFiles { .. }
      | Self::ProbeRepo { .. }
      | Self::GitRemoteInfo { .. }
      | Self::WorkspacePullRequest { .. }
      | Self::GithubRead { .. }
      | Self::AgentStatus { .. }
      | Self::AgentLogs { .. }
      | Self::PtyList { .. }
      | Self::PtyStatus
      | Self::PtyAttachCommand { .. }
      | Self::PtyMsgPoll { .. }
      | Self::WorkspaceChangeMarker { .. }
      | Self::MachineUsage { .. }
      | Self::MachineTools { .. }
      | Self::ListDirectories { .. }
      | Self::ReviewLoad { .. } => false,
      Self::HostCheck { .. } => false,
      // -- Mutations -----------------------------------------------------------
      Self::CloneRepo { .. }
      | Self::InitRepo { .. }
      | Self::NewWorkspaces { .. }
      | Self::CreateWorkspace { .. }
      | Self::RenameWorkspace { .. }
      | Self::UpdateWorkspace { .. }
      | Self::DeleteWorkspace { .. }
      | Self::ArchiveWorkspace { .. }
      | Self::MoveWorkspaceChanges { .. }
      | Self::RebaseWorkspace { .. }
      | Self::SwitchRepoBranch { .. }
      | Self::RestoreFile { .. }
      | Self::PatchFile { .. }
      | Self::SnapshotWorkingCopy { .. }
      | Self::RestoreAll { .. }
      | Self::RestoreSnapshot { .. }
      | Self::StashWorkspaceChanges { .. }
      | Self::CreateCommit { .. }
      | Self::DescribeCommit { .. }
      | Self::SplitCommit { .. }
      | Self::MoveCommit { .. }
      | Self::UndoCommit { .. }
      | Self::AbandonCommit { .. }
      | Self::UndoOperation { .. }
      | Self::RevertCommit { .. }
      | Self::ResolveConflict { .. }
      | Self::GitFetch { .. }
      | Self::PullWorkspace { .. }
      | Self::GitBookmarkTrack { .. }
      | Self::GitPush { .. }
      | Self::CreatePullRequest { .. }
      | Self::GithubWrite { .. }
      | Self::AgentStart { .. }
      | Self::AgentInput { .. }
      | Self::AgentStop { .. }
      | Self::PtyStart { .. }
      | Self::PtyStop { .. }
      | Self::PtyRename { .. }
      | Self::PtyReset { .. }
      | Self::PtyPaste { .. }
      | Self::PtyMsgDeliver { .. }
      | Self::ReviewSave { .. }
      | Self::ReviewClear { .. } => true,
    }
  }

  /// Mutations that are not naturally idempotent must carry a non-empty
  /// idempotency key. Overwrite-style and no-op-on-repeat operations
  /// (describe/update, fetch, bookmark track, restore, delete, agent stop)
  /// do not require one.
  pub fn requires_idempotency_key(&self) -> bool {
    match self {
      Self::CloneRepo { .. }
      | Self::InitRepo { .. }
      | Self::NewWorkspaces { .. }
      | Self::CreateWorkspace { .. }
      | Self::RenameWorkspace { .. }
      | Self::MoveWorkspaceChanges { .. }
      | Self::RebaseWorkspace { .. }
      | Self::PatchFile { .. }
      | Self::StashWorkspaceChanges { .. }
      | Self::CreateCommit { .. }
      | Self::SplitCommit { .. }
      | Self::MoveCommit { .. }
      | Self::AbandonCommit { .. }
      | Self::RevertCommit { .. }
      | Self::ResolveConflict { .. }
      | Self::GitPush { .. }
      | Self::CreatePullRequest { .. }
      | Self::GithubWrite { .. }
      | Self::AgentStart { .. }
      | Self::AgentInput { .. }
      | Self::PtyStart { .. }
      | Self::PtyPaste { .. } => true,
      Self::InspectRepository { .. }
      | Self::RepositoryStatus { .. }
      | Self::ListBranches { .. }
      | Self::ListWorkspaces { .. }
      | Self::WorkspaceSummaries { .. }
      | Self::PlanNewWorkspaces { .. }
      | Self::InspectWorkspace { .. }
      | Self::ListChanges { .. }
      | Self::DiffFile { .. }
      | Self::DiffFiles { .. }
      | Self::ReadFile { .. }
      | Self::ListCommits { .. }
      | Self::ListConflicts { .. }
      | Self::WorkspaceDiff { .. }
      | Self::CommitDiff { .. }
      | Self::CommitFileDiff { .. }
      | Self::SearchFiles { .. }
      | Self::ListFiles { .. }
      | Self::WorkspaceChangeMarker { .. }
      | Self::MachineUsage { .. }
      | Self::MachineTools { .. }
      | Self::ListDirectories { .. }
      | Self::HostCheck { .. }
      | Self::ProbeRepo { .. }
      | Self::UpdateWorkspace { .. }
      | Self::DeleteWorkspace { .. }
      | Self::ArchiveWorkspace { .. }
      | Self::SwitchRepoBranch { .. }
      | Self::RestoreFile { .. }
      | Self::SnapshotWorkingCopy { .. }
      | Self::RestoreAll { .. }
      | Self::RestoreSnapshot { .. }
      | Self::DescribeCommit { .. }
      | Self::UndoOperation { .. }
      | Self::UndoCommit { .. }
      | Self::GitFetch { .. }
      | Self::PullWorkspace { .. }
      | Self::GitBookmarkTrack { .. }
      | Self::GitRemoteInfo { .. }
      | Self::WorkspacePullRequest { .. }
      | Self::GithubRead { .. }
      | Self::AgentStatus { .. }
      | Self::AgentStop { .. }
      | Self::AgentLogs { .. }
      | Self::PtyList { .. }
      | Self::PtyStatus
      | Self::PtyStop { .. }
      | Self::PtyRename { .. }
      | Self::PtyReset { .. }
      | Self::PtyAttachCommand { .. }
      | Self::PtyMsgDeliver { .. }
      | Self::PtyMsgPoll { .. }
      | Self::ReviewLoad { .. }
      | Self::ReviewSave { .. }
      | Self::ReviewClear { .. } => false,
    }
  }

  pub fn kind_name(&self) -> &'static str {
    match self {
      Self::InspectRepository { .. } => "InspectRepository",
      Self::RepositoryStatus { .. } => "RepositoryStatus",
      Self::ListBranches { .. } => "ListBranches",
      Self::ListWorkspaces { .. } => "ListWorkspaces",
      Self::WorkspaceSummaries { .. } => "WorkspaceSummaries",
      Self::PlanNewWorkspaces { .. } => "PlanNewWorkspaces",
      Self::InspectWorkspace { .. } => "InspectWorkspace",
      Self::ListChanges { .. } => "ListChanges",
      Self::DiffFile { .. } => "DiffFile",
      Self::DiffFiles { .. } => "DiffFiles",
      Self::ReadFile { .. } => "ReadFile",
      Self::ListCommits { .. } => "ListCommits",
      Self::ListConflicts { .. } => "ListConflicts",
      Self::WorkspaceDiff { .. } => "WorkspaceDiff",
      Self::CommitDiff { .. } => "CommitDiff",
      Self::CommitFileDiff { .. } => "CommitFileDiff",
      Self::SearchFiles { .. } => "SearchFiles",
      Self::ListFiles { .. } => "ListFiles",
      Self::WorkspaceChangeMarker { .. } => "WorkspaceChangeMarker",
      Self::MachineUsage { .. } => "MachineUsage",
      Self::MachineTools { .. } => "MachineTools",
      Self::ListDirectories { .. } => "ListDirectories",
      Self::HostCheck { .. } => "HostCheck",
      Self::ProbeRepo { .. } => "ProbeRepo",
      Self::CloneRepo { .. } => "CloneRepo",
      Self::InitRepo { .. } => "InitRepo",
      Self::NewWorkspaces { .. } => "NewWorkspaces",
      Self::CreateWorkspace { .. } => "CreateWorkspace",
      Self::RenameWorkspace { .. } => "RenameWorkspace",
      Self::UpdateWorkspace { .. } => "UpdateWorkspace",
      Self::DeleteWorkspace { .. } => "DeleteWorkspace",
      Self::ArchiveWorkspace { .. } => "ArchiveWorkspace",
      Self::MoveWorkspaceChanges { .. } => "MoveWorkspaceChanges",
      Self::RebaseWorkspace { .. } => "RebaseWorkspace",
      Self::SwitchRepoBranch { .. } => "SwitchRepoBranch",
      Self::RestoreFile { .. } => "RestoreFile",
      Self::PatchFile { .. } => "PatchFile",
      Self::SnapshotWorkingCopy { .. } => "SnapshotWorkingCopy",
      Self::RestoreAll { .. } => "RestoreAll",
      Self::RestoreSnapshot { .. } => "RestoreSnapshot",
      Self::StashWorkspaceChanges { .. } => "StashWorkspaceChanges",
      Self::CreateCommit { .. } => "CreateCommit",
      Self::DescribeCommit { .. } => "DescribeCommit",
      Self::SplitCommit { .. } => "SplitCommit",
      Self::MoveCommit { .. } => "MoveCommit",
      Self::UndoCommit { .. } => "UndoCommit",
      Self::AbandonCommit { .. } => "AbandonCommit",
      Self::UndoOperation { .. } => "UndoOperation",
      Self::RevertCommit { .. } => "RevertCommit",
      Self::ResolveConflict { .. } => "ResolveConflict",
      Self::GitFetch { .. } => "GitFetch",
      Self::PullWorkspace { .. } => "PullWorkspace",
      Self::GitBookmarkTrack { .. } => "GitBookmarkTrack",
      Self::GitRemoteInfo { .. } => "GitRemoteInfo",
      Self::GitPush { .. } => "GitPush",
      Self::WorkspacePullRequest { .. } => "WorkspacePullRequest",
      Self::CreatePullRequest { .. } => "CreatePullRequest",
      Self::GithubRead { .. } => "GithubRead",
      Self::GithubWrite { .. } => "GithubWrite",
      Self::AgentStart { .. } => "AgentStart",
      Self::AgentInput { .. } => "AgentInput",
      Self::AgentStatus { .. } => "AgentStatus",
      Self::AgentStop { .. } => "AgentStop",
      Self::AgentLogs { .. } => "AgentLogs",
      Self::PtyStart { .. } => "PtyStart",
      Self::PtyList { .. } => "PtyList",
      Self::PtyStop { .. } => "PtyStop",
      Self::PtyRename { .. } => "PtyRename",
      Self::PtyReset { .. } => "PtyReset",
      Self::PtyStatus => "PtyStatus",
      Self::PtyAttachCommand { .. } => "PtyAttachCommand",
      Self::PtyPaste { .. } => "PtyPaste",
      Self::PtyMsgDeliver { .. } => "PtyMsgDeliver",
      Self::PtyMsgPoll { .. } => "PtyMsgPoll",
      Self::ReviewLoad { .. } => "ReviewLoad",
      Self::ReviewSave { .. } => "ReviewSave",
      Self::ReviewClear { .. } => "ReviewClear",
    }
  }

  /// Stable list of every typed command kind. TypeScript
  /// `TREQ_COMMAND_KINDS` must match this exactly.
  pub const KIND_NAMES: &'static [&'static str] = &[
    "InspectRepository",
    "RepositoryStatus",
    "ListBranches",
    "ListWorkspaces",
    "WorkspaceSummaries",
    "PlanNewWorkspaces",
    "InspectWorkspace",
    "ListChanges",
    "DiffFile",
    "DiffFiles",
    "ReadFile",
    "ListCommits",
    "ListConflicts",
    "WorkspaceChangeMarker",
    "MachineUsage",
    "MachineTools",
    "ListDirectories",
    "HostCheck",
    "ProbeRepo",
    "CloneRepo",
    "InitRepo",
    "NewWorkspaces",
    "CreateWorkspace",
    "RenameWorkspace",
    "UpdateWorkspace",
    "DeleteWorkspace",
    "ArchiveWorkspace",
    "MoveWorkspaceChanges",
    "RebaseWorkspace",
    "SwitchRepoBranch",
    "RestoreFile",
    "PatchFile",
    "SnapshotWorkingCopy",
    "RestoreAll",
    "RestoreSnapshot",
    "StashWorkspaceChanges",
    "CreateCommit",
    "DescribeCommit",
    "SplitCommit",
    "MoveCommit",
    "UndoCommit",
    "AbandonCommit",
    "UndoOperation",
    "RevertCommit",
    "ResolveConflict",
    "GitFetch",
    "PullWorkspace",
    "GitBookmarkTrack",
    "GitRemoteInfo",
    "GitPush",
    "AgentStart",
    "AgentInput",
    "AgentStatus",
    "AgentStop",
    "AgentLogs",
    "PtyStart",
    "PtyList",
    "PtyStop",
    "PtyRename",
    "PtyReset",
    "PtyStatus",
    "PtyAttachCommand",
    "PtyPaste",
    "PtyMsgDeliver",
    "PtyMsgPoll",
    "ReviewLoad",
    "ReviewSave",
    "ReviewClear",
    "WorkspaceDiff",
    "CommitDiff",
    "CommitFileDiff",
    "SearchFiles",
    "ListFiles",
    "WorkspacePullRequest",
    "CreatePullRequest",
    "GithubRead",
    "GithubWrite",
  ];
}

/// Packs a `pty-remote` launch spec into `--value`'s single string slot as
/// small JSON, since `CliArgFields` has no dedicated cols/rows/remote_dir
/// slots. Parsed back by `parse_pty_launch_payload` in `cli::mod`.
#[derive(Serialize, Deserialize)]
pub struct PtyLaunchPayload {
  pub remote_dir: String,
  pub launch: crate::core::remote_pty::PtyLaunchSpec,
  pub cols: u16,
  pub rows: u16,
}

#[expect(
  clippy::expect_used,
  reason = "the payload is plain data that always serializes"
)]
pub fn pty_launch_payload(
  remote_dir: &str,
  launch: &crate::core::remote_pty::PtyLaunchSpec,
  cols: u16,
  rows: u16,
) -> String {
  serde_json::to_string(&PtyLaunchPayload {
    remote_dir: remote_dir.to_string(),
    launch: launch.clone(),
    cols,
    rows,
  })
  .expect("PtyLaunchPayload serializes")
}

/// Parses the `--value` payload `pty_launch_payload` produced. Public so the
/// CLI dispatch layer (`cli::mod::parse_remote_command_request`) can decode
/// it without duplicating the JSON shape.
pub fn parse_pty_launch_payload(value: &str) -> Result<PtyLaunchPayload, String> {
  serde_json::from_str(value)
    .map_err(|e| format!("invalid_arguments: malformed pty launch payload: {e}"))
}

/// Allow-listed argument fields shared by every Phase 5 typed command. Only
/// commands built from a [`TreqCommandRequest`] variant can populate these —
/// no frontend-provided raw string is ever assembled into this shape, which
/// is what keeps arbitrary commands out of the exec channel.
#[derive(Default)]
struct CliArgFields<'a> {
  workspace: Option<&'a str>,
  path: Option<&'a str>,
  owned_path: Option<String>,
  target: Option<&'a str>,
  value: Option<String>,
  idempotency_key: Option<&'a str>,
  /// Additional named flags (`--<name> <value>`) for request fields that do
  /// not fit the shared slots above. Each name must be declared for the
  /// matching subcommand in `tauri.conf.json`.
  extra: Vec<(&'static str, String)>,
}

fn hunk_specs_arg(hunks: &[crate::core::workspaces::HunkSpec]) -> Option<String> {
  if hunks.is_empty() {
    return None;
  }
  Some(
    hunks
      .iter()
      .map(|h| format!("{}:{}-{}", h.file_path, h.start_line, h.end_line))
      .collect::<Vec<_>>()
      .join(","),
  )
}

/// Adds `--backend` for a machine on tmux; the daemon is the CLI's default.
fn push_backend(
  fields: &mut CliArgFields,
  backend: crate::core::pty_remote_supervisor::PtyBackend,
) {
  if !backend.is_default() {
    fields.extra.push(("backend", backend.as_str().to_string()));
  }
}

impl TreqCommandRequest {
  /// Convert a typed request into remote CLI arguments. Values remain separate
  /// process arguments so no frontend-provided value is interpreted as shell.
  pub fn cli_args(&self) -> Result<Vec<String>, String> {
    let mut fields = CliArgFields::default();
    let (command, action, repo): (&str, &str, &str) = match self {
      Self::InspectRepository { repo } => ("repo", "inspect", repo),
      Self::RepositoryStatus { repo } => ("repo", "status", repo),
      Self::ListBranches { repo } => ("repo", "branches", repo),
      Self::ProbeRepo { repo } => ("repo", "probe", repo),
      Self::MachineUsage { root } => ("repo", "usage", root),
      Self::MachineTools { root } => ("repo", "tools", root),
      Self::ListDirectories { path } => ("repo", "dirs", path),
      Self::HostCheck { root } => ("repo", "host-check", root),
      Self::InitRepo {
        repo,
        idempotency_key,
      } => {
        fields.idempotency_key = Some(idempotency_key);
        ("repo", "init", repo)
      }
      Self::CloneRepo {
        repo_url,
        destination,
        idempotency_key,
      } => {
        if repo_url.trim().is_empty() {
          return Err("Repository URL is required".to_string());
        }
        fields.value = Some(repo_url.clone());
        fields.idempotency_key = Some(idempotency_key);
        ("repo", "clone", destination)
      }
      Self::ListWorkspaces { repo } => ("workspace", "list", repo),
      Self::WorkspaceSummaries { repo } => ("workspace", "summaries", repo),
      Self::InspectWorkspace { repo, workspace } => {
        fields.workspace = Some(workspace);
        ("workspace", "inspect", repo)
      }
      Self::PlanNewWorkspaces {
        repo,
        text,
        stack_on,
      } => {
        fields.value = Some(text.clone());
        fields.target = stack_on.as_deref();
        ("workspace", "plan-new", repo)
      }
      Self::NewWorkspaces {
        repo,
        text,
        stack_on,
        idempotency_key,
      } => {
        fields.value = Some(text.clone());
        fields.target = stack_on.as_deref();
        fields.idempotency_key = Some(idempotency_key);
        ("workspace", "new", repo)
      }
      Self::CreateWorkspace {
        repo,
        branch_name,
        source_branch,
        metadata,
        idempotency_key,
      } => {
        fields.value = Some(branch_name.clone());
        fields.target = source_branch.as_deref();
        if let Some(metadata) = metadata.as_ref().filter(|m| !m.trim().is_empty()) {
          fields.extra.push(("metadata", metadata.clone()));
        }
        fields.idempotency_key = Some(idempotency_key);
        ("workspace", "create", repo)
      }
      Self::RenameWorkspace {
        repo,
        workspace,
        new_name,
        idempotency_key,
      } => {
        fields.workspace = Some(workspace);
        fields.value = Some(new_name.clone());
        fields.idempotency_key = Some(idempotency_key);
        ("workspace", "rename", repo)
      }
      Self::UpdateWorkspace {
        repo,
        workspace,
        target_branch,
        title,
        description,
      } => {
        fields.workspace = Some(workspace);
        fields.target = target_branch.as_deref();
        fields.value = description.clone();
        if let Some(title) = title {
          fields.extra.push(("title", title.clone()));
        }
        ("workspace", "update", repo)
      }
      Self::DeleteWorkspace { repo, workspace } => {
        fields.workspace = Some(workspace);
        ("workspace", "delete", repo)
      }
      Self::ArchiveWorkspace { repo, workspace } => {
        fields.workspace = Some(workspace);
        ("workspace", "archive", repo)
      }
      Self::MoveWorkspaceChanges {
        repo,
        workspace,
        destination,
        files,
        hunks,
        commits,
        idempotency_key,
      } => {
        fields.workspace = Some(workspace);
        fields.target = Some(destination);
        if !commits.is_empty() {
          fields.value = Some(commits.join(","));
        }
        if !files.is_empty() {
          fields.extra.push(("files", files.join(",")));
        }
        fields.owned_path = hunk_specs_arg(hunks);
        fields.idempotency_key = Some(idempotency_key);
        ("workspace", "move", repo)
      }
      Self::RebaseWorkspace {
        repo,
        workspace,
        target_branch,
        idempotency_key,
      } => {
        fields.workspace = Some(workspace);
        fields.target = Some(target_branch);
        fields.idempotency_key = Some(idempotency_key);
        ("workspace", "rebase", repo)
      }
      Self::SwitchRepoBranch { repo, bookmark } => {
        fields.value = Some(bookmark.clone());
        ("repo", "switch-branch", repo)
      }
      Self::ListChanges { repo, workspace } => {
        fields.workspace = workspace.as_deref();
        ("changes", "list", repo)
      }
      Self::WorkspaceDiff { repo, workspace } => {
        fields.workspace = Some(workspace);
        ("changes", "workspace-diff", repo)
      }
      Self::CommitDiff {
        repo,
        workspace,
        revision,
      } => {
        fields.workspace = workspace.as_deref();
        fields.target = Some(revision);
        ("commits", "diff", repo)
      }
      Self::CommitFileDiff {
        repo,
        workspace,
        revision,
        path,
      } => {
        fields.workspace = workspace.as_deref();
        fields.target = Some(revision);
        fields.path = Some(path);
        ("commits", "file-diff", repo)
      }
      Self::SearchFiles {
        repo,
        workspace,
        query,
        limit,
      } => {
        fields.workspace = workspace.as_deref();
        fields.value = Some(query.clone());
        if let Some(limit) = limit {
          fields.extra.push(("limit", limit.to_string()));
        }
        ("file", "search", repo)
      }
      Self::ListFiles {
        repo,
        workspace,
        path,
      } => {
        fields.workspace = workspace.as_deref();
        // The workspace root is an empty path, which `--path` refuses.
        if !path.is_empty() {
          fields.path = Some(path);
        }
        ("file", "list", repo)
      }
      Self::DiffFile {
        repo,
        workspace,
        path,
      } => {
        fields.workspace = workspace.as_deref();
        fields.path = Some(path);
        ("changes", "diff", repo)
      }
      Self::DiffFiles {
        repo,
        workspace,
        paths,
        token,
      } => {
        fields.workspace = workspace.as_deref();
        // JSON, as paths may hold commas.
        let paths = serde_json::to_string(paths).map_err(|e| e.to_string())?;
        fields.extra.push(("paths", paths));
        if let Some(token) = token {
          fields.extra.push(("token", token.clone()));
        }
        ("changes", "diff-files", repo)
      }
      Self::ReadFile {
        repo,
        workspace,
        path,
        ..
      } => {
        fields.workspace = workspace.as_deref();
        fields.path = Some(path);
        ("file", "read", repo)
      }
      Self::RestoreFile {
        repo,
        workspace,
        path,
      } => {
        fields.workspace = workspace.as_deref();
        fields.path = Some(path);
        ("file", "restore", repo)
      }
      Self::PatchFile {
        repo,
        workspace,
        path,
        patch_base64,
        idempotency_key,
      } => {
        fields.workspace = workspace.as_deref();
        fields.path = Some(path);
        fields.value = Some(patch_base64.clone());
        fields.idempotency_key = Some(idempotency_key);
        ("file", "patch", repo)
      }
      Self::SnapshotWorkingCopy { repo, workspace } => {
        fields.workspace = workspace.as_deref();
        ("workspace", "snapshot", repo)
      }
      Self::RestoreAll { repo, workspace } => {
        fields.workspace = workspace.as_deref();
        ("workspace", "restore-all", repo)
      }
      Self::RestoreSnapshot {
        repo,
        workspace,
        snapshot_id,
      } => {
        fields.workspace = workspace.as_deref();
        fields.target = Some(snapshot_id);
        ("workspace", "restore-snapshot", repo)
      }
      Self::StashWorkspaceChanges {
        repo,
        workspace,
        idempotency_key,
      } => {
        fields.workspace = workspace.as_deref();
        fields.idempotency_key = Some(idempotency_key);
        ("workspace", "stash", repo)
      }
      Self::ListCommits { repo, workspace } => {
        fields.workspace = workspace.as_deref();
        ("commits", "list", repo)
      }
      Self::CreateCommit {
        repo,
        workspace,
        message,
        base_change_id,
        idempotency_key,
      } => {
        fields.workspace = workspace.as_deref();
        fields.value = Some(message.clone());
        fields.target = base_change_id.as_deref();
        fields.idempotency_key = Some(idempotency_key);
        ("commits", "create", repo)
      }
      Self::DescribeCommit {
        repo,
        workspace,
        commit,
        message,
      } => {
        fields.workspace = Some(workspace);
        fields.target = Some(commit);
        fields.value = Some(message.clone());
        ("commits", "describe", repo)
      }
      Self::SplitCommit {
        repo,
        workspace,
        commit,
        files,
        hunks,
        message,
        idempotency_key,
      } => {
        if files.iter().all(|f| f.trim().is_empty()) && hunks.is_empty() {
          return Err(
            "invalid_arguments: SplitCommit requires selected files or hunks".to_string(),
          );
        }
        fields.workspace = Some(workspace);
        fields.target = Some(commit);
        if !files.is_empty() {
          fields.value = Some(files.join(","));
        }
        fields.owned_path = hunk_specs_arg(hunks);
        if !message.is_empty() {
          fields.extra.push(("message", message.clone()));
        }
        fields.idempotency_key = Some(idempotency_key);
        ("commits", "split", repo)
      }
      Self::MoveCommit {
        repo,
        workspace,
        commit,
        target_workspace,
        idempotency_key,
      } => {
        fields.workspace = Some(workspace);
        fields.target = Some(commit);
        fields.value = Some(target_workspace.clone());
        fields.idempotency_key = Some(idempotency_key);
        ("commits", "move", repo)
      }
      Self::UndoCommit {
        repo,
        workspace,
        commit,
      } => {
        fields.workspace = Some(workspace);
        fields.target = Some(commit);
        ("commits", "undo", repo)
      }
      Self::AbandonCommit {
        repo,
        workspace,
        commit,
        idempotency_key,
      } => {
        fields.workspace = Some(workspace);
        fields.target = Some(commit);
        fields.idempotency_key = Some(idempotency_key);
        ("commits", "abandon", repo)
      }
      Self::UndoOperation {
        repo,
        workspace,
        operation_id,
      } => {
        fields.workspace = workspace.as_deref();
        fields.target = Some(operation_id);
        ("commits", "undo-operation", repo)
      }
      Self::RevertCommit {
        repo,
        workspace,
        commit,
        idempotency_key,
      } => {
        fields.workspace = Some(workspace);
        fields.target = Some(commit);
        fields.idempotency_key = Some(idempotency_key);
        ("commits", "revert", repo)
      }
      Self::ListConflicts { repo, workspace } => {
        fields.workspace = workspace.as_deref();
        ("conflicts", "list", repo)
      }
      Self::WorkspaceChangeMarker { repo, workspace } => {
        fields.workspace = workspace.as_deref();
        ("workspace", "marker", repo)
      }
      Self::ResolveConflict {
        repo,
        revision,
        sides,
        idempotency_key,
      } => {
        fields.target = Some(revision);
        fields.value = Some(sides.join(","));
        fields.idempotency_key = Some(idempotency_key);
        ("conflicts", "resolve", repo)
      }
      Self::GitFetch { repo } => ("git", "fetch", repo),
      Self::PullWorkspace { repo, workspace } => {
        fields.workspace = workspace.as_deref();
        ("git", "pull", repo)
      }
      Self::GitRemoteInfo { repo } => ("git", "remote-info", repo),
      Self::GitBookmarkTrack {
        repo,
        bookmark,
        remote_name,
      } => {
        fields.value = Some(bookmark.clone());
        fields.target = Some(remote_name);
        ("git", "bookmark-track", repo)
      }
      Self::GitPush {
        repo,
        workspace,
        idempotency_key,
      } => {
        fields.workspace = workspace.as_deref();
        fields.idempotency_key = Some(idempotency_key);
        ("git", "push", repo)
      }
      Self::WorkspacePullRequest { repo, workspace } => {
        fields.workspace = Some(workspace);
        ("git", "pr", repo)
      }
      Self::CreatePullRequest {
        repo,
        workspace,
        title,
        body,
        draft,
        idempotency_key,
      } => {
        fields.workspace = Some(workspace);
        fields.value = Some(body.clone());
        fields.extra.push(("title", title.clone()));
        if *draft {
          fields.extra.push(("draft", "true".into()));
        }
        fields.idempotency_key = Some(idempotency_key);
        ("git", "pr-create", repo)
      }
      Self::GithubRead { repo, query } => {
        fields.value = Some(serde_json::to_string(query).map_err(|e| e.to_string())?);
        ("git", "github-read", repo)
      }
      Self::GithubWrite {
        repo,
        action,
        idempotency_key,
      } => {
        fields.value = Some(serde_json::to_string(action).map_err(|e| e.to_string())?);
        fields.idempotency_key = Some(idempotency_key);
        ("git", "github-write", repo)
      }
      Self::AgentStart {
        repo,
        workspace,
        agent,
        prompt,
        idempotency_key,
      } => {
        fields.workspace = Some(workspace);
        fields.target = Some(agent);
        fields.value = Some(prompt.clone());
        fields.idempotency_key = Some(idempotency_key);
        ("agent-remote", "start", repo)
      }
      Self::AgentInput {
        repo,
        workspace,
        input,
        idempotency_key,
      } => {
        fields.workspace = Some(workspace);
        fields.value = Some(input.clone());
        fields.idempotency_key = Some(idempotency_key);
        ("agent-remote", "input", repo)
      }
      Self::AgentStatus { repo, workspace } => {
        fields.workspace = Some(workspace);
        ("agent-remote", "status", repo)
      }
      Self::AgentStop { repo, workspace } => {
        fields.workspace = Some(workspace);
        ("agent-remote", "stop", repo)
      }
      Self::AgentLogs { repo, workspace } => {
        fields.workspace = Some(workspace);
        ("agent-remote", "logs", repo)
      }
      Self::PtyStart {
        backend,
        repo,
        workspace,
        label,
        remote_dir,
        launch,
        cols,
        rows,
        idempotency_key,
      } => {
        push_backend(&mut fields, *backend);
        fields.workspace = Some(workspace);
        fields.target = Some(label);
        fields.value = Some(pty_launch_payload(remote_dir, launch, *cols, *rows));
        fields.idempotency_key = Some(idempotency_key);
        ("pty-remote", "start", repo)
      }
      Self::PtyList {
        backend,
        repo,
        workspace,
      } => {
        push_backend(&mut fields, *backend);
        fields.workspace = workspace.as_deref();
        ("pty-remote", "list", repo)
      }
      Self::PtyStatus => ("pty-remote", "status", "/"),
      Self::PtyReset { backend } => {
        push_backend(&mut fields, *backend);
        // Reset reaches every repository on the host; `--repo` only
        // satisfies the CLI's shared argument shape.
        ("pty-remote", "reset", "/")
      }
      Self::PtyStop {
        backend,
        repo,
        workspace,
        label,
      } => {
        push_backend(&mut fields, *backend);
        fields.workspace = Some(workspace);
        fields.target = Some(label);
        ("pty-remote", "stop", repo)
      }
      Self::PtyRename {
        backend,
        repo,
        workspace,
        label,
        title,
      } => {
        push_backend(&mut fields, *backend);
        fields.workspace = Some(workspace);
        fields.target = Some(label);
        fields.value = Some(title.clone());
        ("pty-remote", "rename", repo)
      }
      Self::PtyAttachCommand {
        backend,
        repo,
        workspace,
        label,
        remote_dir,
        launch,
        cols,
        rows,
      } => {
        push_backend(&mut fields, *backend);
        fields.workspace = Some(workspace);
        fields.target = Some(label);
        fields.value = Some(pty_launch_payload(remote_dir, launch, *cols, *rows));
        ("pty-remote", "attach-command", repo)
      }
      Self::PtyPaste {
        backend,
        repo,
        workspace,
        label,
        text,
        submit,
        idempotency_key,
      } => {
        push_backend(&mut fields, *backend);
        fields.workspace = Some(workspace);
        fields.target = Some(label);
        fields.value = Some(text.clone());
        fields.idempotency_key = Some(idempotency_key);
        if *submit {
          fields.extra.push(("submit", "true".into()));
        }
        ("pty-remote", "paste", repo)
      }
      Self::PtyMsgDeliver {
        repo,
        workspace,
        label,
        envelope,
      } => {
        fields.workspace = Some(workspace);
        fields.target = Some(label);
        fields.value = Some(envelope.clone());
        ("pty-remote", "msg-deliver", repo)
      }
      Self::PtyMsgPoll {
        repo,
        workspace,
        label,
        since,
      } => {
        fields.workspace = Some(workspace);
        fields.target = Some(label);
        fields.value = Some(since.to_string());
        ("pty-remote", "msg-poll", repo)
      }
      Self::ReviewLoad {
        repo,
        workspace,
        scope,
      } => {
        fields.workspace = Some(workspace);
        fields.target = Some(scope.as_str());
        ("changes", "review", repo)
      }
      Self::ReviewSave {
        repo,
        workspace,
        scope,
        review,
      } => {
        fields.workspace = Some(workspace);
        fields.target = Some(scope.as_str());
        fields.value =
          Some(serde_json::to_string(review).map_err(|e| format!("Failed to encode review: {e}"))?);
        ("changes", "review-save", repo)
      }
      Self::ReviewClear {
        repo,
        workspace,
        scope,
      } => {
        fields.workspace = Some(workspace);
        fields.target = Some(scope.as_str());
        ("changes", "review-clear", repo)
      }
    };
    validate_remote_path(repo)?;
    let path = fields.path.or(fields.owned_path.as_deref());
    if let Some(path) = path {
      if path.trim().is_empty() {
        return Err("Remote file path is required".to_string());
      }
    }
    let mut args = vec![
      command.into(),
      action.into(),
      "--repo".into(),
      repo.to_string(),
    ];
    if let Some(workspace) = fields.workspace {
      if workspace.trim().is_empty() {
        return Err("Remote workspace is required".to_string());
      }
      args.extend(["--workspace".into(), workspace.to_string()]);
    }
    if let Some(path) = path {
      args.extend(["--path".into(), path.to_string()]);
    }
    if let Some(target) = fields.target {
      args.extend(["--target".into(), target.to_string()]);
    }
    if let Some(value) = &fields.value {
      args.extend(["--value".into(), value.clone()]);
    }
    for (name, value) in &fields.extra {
      args.extend([format!("--{name}"), value.clone()]);
    }
    if self.requires_idempotency_key() {
      match fields.idempotency_key {
        Some(key) if !key.trim().is_empty() => {
          args.extend(["--idempotency-key".into(), key.to_string()]);
        }
        _ => {
          return Err("invalid_arguments: idempotency key is required".to_string());
        }
      }
    } else if let Some(key) = fields.idempotency_key {
      if key.trim().is_empty() {
        return Err("Idempotency key must not be empty when provided".to_string());
      }
      args.extend(["--idempotency-key".into(), key.to_string()]);
    }
    if let Self::ReadFile {
      revision,
      start_line,
      end_line,
      ..
    } = self
    {
      args.extend([
        "--revision".into(),
        match revision {
          FileRevision::WorkingCopy => "working-copy",
          FileRevision::Parent => "parent",
        }
        .into(),
      ]);
      if let Some(line) = start_line {
        args.extend(["--start-line".into(), line.to_string()]);
      }
      if let Some(line) = end_line {
        args.extend(["--end-line".into(), line.to_string()]);
      }
    }
    args.extend(["--format".into(), "json".into()]);
    Ok(args)
  }
}

/// Structured error for a typed command executed over the native transport.
/// Distinguishes a transport-layer failure (connection/host-key/auth/limits —
/// see [`crate::core::remote_ssh_transport::SshTransportError`]) from a
/// command-layer failure the remote Treq CLI itself reported, so a caller
/// can tell "we could not reach the CLI" from "the CLI ran and rejected the
/// request" and keep the CLI's own error code intact either way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteCommandError {
  Transport(String),
  Command {
    code: String,
    message: String,
  },
  InvalidJson(String),
  /// The endpoint's credential is under hard cutoff (PRD "Hard cutoff on
  /// revocation or expiry") — no further structured commands may be sent
  /// until the user reauthenticates. Kept distinct from `Transport` so a
  /// caller (the Tauri command layer, the UI) can drive a reauth prompt
  /// instead of a generic transport-error toast.
  CredentialCutOff {
    endpoint_id: String,
    reason: String,
  },
}

impl std::fmt::Display for RemoteCommandError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::Transport(message) => write!(f, "transport_error: {message}"),
      Self::Command { code, message } => write!(f, "{code}: {message}"),
      Self::InvalidJson(message) => write!(f, "invalid_remote_json: {message}"),
      Self::CredentialCutOff {
        endpoint_id,
        reason,
      } => {
        write!(f, "credential_cut_off: endpoint {endpoint_id} ({reason})")
      }
    }
  }
}

impl std::error::Error for RemoteCommandError {}

/// Executes a typed `TreqCommandRequest` against `endpoint` over the pooled
/// native SSH transport's exec channel and decodes the JSON result. This is
/// the only path by which a Phase 5 mutation, read-only command, probe,
/// clone, or agent lifecycle call reaches the wire — `request` is always
/// built from an allow-listed enum variant, never a raw string, so no
/// frontend-provided arbitrary command can enter the exec channel.
pub async fn execute_remote_command<T: DeserializeOwned>(
  pool: &crate::core::remote_ssh_transport::SshConnectionPool,
  endpoint: &crate::core::remote_control_plane::SshEndpoint,
  request: TreqCommandRequest,
  limits: crate::core::remote_ssh_transport::ExecLimits,
  cancellation: &crate::core::remote_ssh_transport::CancellationToken,
) -> Result<T, RemoteCommandError> {
  let args = request
    .cli_args()
    .map_err(|error| RemoteCommandError::Command {
      code: "invalid_arguments".to_string(),
      message: error,
    })?;
  let output =
    crate::core::remote_ssh_transport::exec_command(pool, endpoint, &args, limits, cancellation)
      .await
      .map_err(|error| match error {
        crate::core::remote_ssh_transport::SshTransportError::CredentialCutOff {
          endpoint_id,
          reason,
        } => RemoteCommandError::CredentialCutOff {
          endpoint_id,
          reason: reason.to_string(),
        },
        crate::core::remote_ssh_transport::SshTransportError::CommandFailed {
          stderr,
          stdout,
          ..
        } => {
          // The CLI always emits `{"error":{"code":...,"message":...}}` on
          // stdout for a structured failure, so prefer that over stderr: it is
          // what lets the CLI's own error code survive transport mapping rather
          // than collapsing to a generic string.
          if let Ok(body) = serde_json::from_str::<CliErrorBody>(&stdout) {
            RemoteCommandError::Command {
              code: body.error.code,
              message: body.error.message,
            }
          } else {
            RemoteCommandError::Command {
              code: "treq_command_failed".to_string(),
              message: if stderr.is_empty() {
                "Remote treq command failed".to_string()
              } else {
                stderr
              },
            }
          }
        }
        other => RemoteCommandError::Transport(other.to_string()),
      })?;
  serde_json::from_slice::<T>(&output.stdout)
    .map_err(|error| RemoteCommandError::InvalidJson(error.to_string()))
}

impl RemoteCommandError {
  /// Whether the command failed because the pooled connection was dead
  /// without being known to be: a channel error (closed session, relay
  /// recycled), a channel that never opened, or the deadline. A read is safe
  /// to send again on a fresh connection.
  pub fn is_stale_connection(&self) -> bool {
    use crate::core::remote_ssh_transport::SshTransportError;
    matches!(self, Self::Transport(message)
      if message.starts_with(&SshTransportError::ChannelError(String::new()).to_string())
        || *message == SshTransportError::DeadlineExceeded.to_string())
  }
}

/// [`execute_remote_command`] for a read: after a stale-connection failure
/// (see [`RemoteCommandError::is_stale_connection`]) the dead connection is
/// dropped and the read is sent once more on a fresh one. Mutations never go
/// through here; they verify before any retry ([`retry_after_reconnect`]).
pub async fn execute_remote_read<T: DeserializeOwned>(
  pool: &crate::core::remote_ssh_transport::SshConnectionPool,
  endpoint: &crate::core::remote_control_plane::SshEndpoint,
  request: TreqCommandRequest,
  limits: crate::core::remote_ssh_transport::ExecLimits,
  cancellation: &crate::core::remote_ssh_transport::CancellationToken,
) -> Result<T, RemoteCommandError> {
  match execute_remote_command::<T>(pool, endpoint, request.clone(), limits, cancellation).await {
    Err(error) if error.is_stale_connection() => {
      pool.mark_dead(endpoint).await;
      execute_remote_command::<T>(pool, endpoint, request, limits, cancellation).await
    }
    result => result,
  }
}

impl TransportError {
  fn command_failed_from_message(message: String) -> Self {
    Self::CommandFailed {
      code: None,
      message,
    }
  }
}

trait TransportErrorExt<T> {
  fn map_err_command(self) -> Result<T, TransportError>;
}

impl<T> TransportErrorExt<T> for Result<T, String> {
  fn map_err_command(self) -> Result<T, TransportError> {
    self.map_err(TransportError::command_failed_from_message)
  }
}

mod verification;
pub use verification::*;
mod local;
pub use local::*;
mod github;
pub use github::*;
mod host;
pub use host::*;
mod ssh;
pub use ssh::*;

#[cfg(test)]
mod tests {
  use super::*;
  use std::process::Command;

  #[test]
  fn reviews_round_trip_per_page_and_an_empty_save_clears() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().to_str().unwrap();
    let id = crate::local_db::add_workspace(
      repo,
      "ws".into(),
      format!("{repo}/.treq/workspaces/ws"),
      "ws".into(),
      None,
      None,
      None,
    )
    .unwrap();
    let review = |summary: &str| crate::core::review::Stored {
      comments: vec![crate::core::review::Pending::file("a.rs").comment(
        "tidy",
        "c1".into(),
        "now".into(),
      )],
      summary: summary.into(),
      notes: vec![],
    };

    save_review(repo, id, ReviewScope::Changes, &review("changes")).unwrap();
    save_review(repo, id, ReviewScope::Files, &review("files")).unwrap();
    assert_eq!(
      load_review(repo, id, ReviewScope::Changes).unwrap(),
      review("changes")
    );
    assert_eq!(
      load_review(repo, id, ReviewScope::Files).unwrap(),
      review("files")
    );

    save_review(repo, id, ReviewScope::Files, &Default::default()).unwrap();
    assert!(load_review(repo, id, ReviewScope::Files)
      .unwrap()
      .is_empty());
    assert!(!load_review(repo, id, ReviewScope::Changes)
      .unwrap()
      .is_empty());
  }

  #[test]
  fn create_pull_request_needs_a_title_before_touching_the_repo() {
    let error = create_pull_request("/nonexistent", 1, "  ", "", false).unwrap_err();
    assert!(error.contains("needs a title"), "{error}");
  }

  #[test]
  fn github_writes_are_keyed_mutations_and_reads_are_not() {
    let write = TreqCommandRequest::GithubWrite {
      repo: "/r".into(),
      action: GithubAction::CloseIssue {
        number: 1,
        reason: "completed".into(),
      },
      idempotency_key: "k".into(),
    };
    assert!(write.is_mutation());
    assert!(write.requires_idempotency_key());
    let read = TreqCommandRequest::GithubRead {
      repo: "/r".into(),
      query: GithubQuery::Issue { number: 1 },
    };
    assert!(!read.is_mutation());
    assert!(!read.requires_idempotency_key());
  }

  #[test]
  fn github_requests_need_a_github_remote_before_running_gh() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().to_str().unwrap();
    let error = github_read(repo, GithubQuery::Issue { number: 1 }).unwrap_err();
    assert!(error.contains("no GitHub remote"), "{error}");
    let error = github_write(repo, GithubAction::ReopenIssue { number: 1 }).unwrap_err();
    assert!(error.contains("no GitHub remote"), "{error}");
  }

  #[test]
  fn github_writes_with_text_reject_a_blank_body_or_title() {
    for action in [
      GithubAction::CreateIssue {
        title: " ".into(),
        body: String::new(),
      },
      GithubAction::CommentOnIssue {
        number: 1,
        body: "\n".into(),
      },
      GithubAction::ReplyToReviewThread {
        thread_id: "t".into(),
        body: "".into(),
      },
    ] {
      let error = github_write("/nonexistent", action).unwrap_err();
      assert!(error.starts_with("invalid_arguments:"), "{error}");
    }
  }

  #[test]
  fn list_files_lists_one_directory_without_ignored_or_hidden_directories() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::process::Command::new("git")
      .args(["init", "-q"])
      .current_dir(root)
      .status()
      .unwrap();
    for sub in ["src/ui", "target", ".github"] {
      fs::create_dir_all(root.join(sub)).unwrap();
    }
    fs::write(root.join(".gitignore"), "target/\n").unwrap();
    fs::write(root.join("README.md"), "").unwrap();
    fs::write(root.join("src/main.rs"), "").unwrap();
    let repo = root.to_str().unwrap().to_string();

    let names = |path: &str| -> Vec<(String, String, bool)> {
      let value = execute_local_request(TreqCommandRequest::ListFiles {
        repo: repo.clone(),
        workspace: None,
        path: path.into(),
      })
      .unwrap();
      serde_json::from_value::<Vec<RemoteFileEntry>>(value)
        .unwrap()
        .into_iter()
        .map(|e| (e.name, e.path, e.is_directory))
        .collect()
    };
    assert_eq!(
      names(""),
      vec![
        ("src".into(), "src".into(), true),
        (".gitignore".into(), ".gitignore".into(), false),
        ("README.md".into(), "README.md".into(), false),
      ]
    );
    assert_eq!(
      names("src/"),
      vec![
        ("ui".into(), "src/ui".into(), true),
        ("main.rs".into(), "src/main.rs".into(), false),
      ]
    );
    for outside in ["..", "../etc", "/etc", "src/../.."] {
      assert!(
        execute_local_request(TreqCommandRequest::ListFiles {
          repo: repo.clone(),
          workspace: None,
          path: outside.into(),
        })
        .is_err(),
        "{outside}"
      );
    }
  }

  #[cfg(unix)]
  #[test]
  fn list_directories_lists_subdirectories_by_name_and_marks_repositories() {
    let dir = tempfile::tempdir().unwrap();
    for sub in ["beta", "Alpha/.git", ".config", "plain/.jj"] {
      fs::create_dir_all(dir.path().join(sub)).unwrap();
    }
    fs::write(dir.path().join("notes.txt"), "").unwrap();
    std::os::unix::fs::symlink(dir.path().join("beta"), dir.path().join("link")).unwrap();

    let listing = list_directories(dir.path().to_str().unwrap()).unwrap();
    let root = dir.path().canonicalize().unwrap();
    assert_eq!(listing.path, root.to_string_lossy());
    assert_eq!(
      listing.parent.as_deref(),
      root.parent().map(|p| p.to_string_lossy()).as_deref()
    );
    assert!(!listing.is_repo);
    let names: Vec<_> = listing
      .entries
      .iter()
      .map(|e| (e.name.as_str(), e.hidden, e.is_repo))
      .collect();
    assert_eq!(
      names,
      [
        (".config", true, false),
        ("Alpha", false, true),
        ("beta", false, false),
        ("link", false, false),
        ("plain", false, true),
      ]
    );
    assert!(
      list_directories(dir.path().join("Alpha").to_str().unwrap())
        .unwrap()
        .is_repo
    );
    assert!(list_directories(dir.path().join("gone").to_str().unwrap()).is_err());
  }

  #[test]
  fn workspace_id_rejects_zero_and_negative_ids() {
    assert_eq!(workspace_id(Some(&"3".to_string())), Ok(Some(3)));
    for bad in ["0", "-1"] {
      let error = workspace_id(Some(&bad.to_string())).unwrap_err();
      assert!(error.contains("positive"), "{bad}: {error}");
    }
  }

  #[test]
  fn read_file_defaults_end_line_to_a_300_line_window_from_start_line() {
    let repo_dir = tempfile::tempdir().unwrap();
    let content: String = (1..=700).map(|n| format!("{n}\n")).collect();
    std::fs::write(repo_dir.path().join("long.txt"), content).unwrap();
    let read = |start_line: Option<usize>| {
      let value = execute_local_request(TreqCommandRequest::ReadFile {
        repo: repo_dir.path().to_str().unwrap().to_string(),
        workspace: None,
        path: "long.txt".into(),
        revision: FileRevision::WorkingCopy,
        start_line,
        end_line: None,
      })
      .unwrap();
      (value["start_line"].clone(), value["end_line"].clone())
    };

    assert_eq!(read(None), (1.into(), 300.into()));
    assert_eq!(read(Some(301)), (301.into(), 600.into()));
  }

  #[test]
  fn parses_ssh_hosts_ignoring_patterns() {
    let hosts = parse_ssh_config_hosts("\nHost prod bastion\n  HostName example.com\nHost *\nHost !blocked *.internal test?\nHost dev\n");
    let aliases: Vec<_> = hosts.into_iter().map(|h| h.alias).collect();
    assert_eq!(aliases, vec!["bastion", "dev", "prod"]);
  }

  #[test]
  fn lists_configured_hosts_from_multiple_files() {
    let temp_dir = tempfile::tempdir().unwrap();
    let user_config = temp_dir.path().join("user_config");
    let system_config = temp_dir.path().join("system_config");
    std::fs::write(&user_config, "Host dev prod\n  User test\n").unwrap();
    std::fs::write(&system_config, "Host prod staging *.ignored\n").unwrap();

    let hosts = list_configured_hosts_from_paths(vec![user_config, system_config]).unwrap();
    let aliases: Vec<_> = hosts.into_iter().map(|host| host.alias).collect();

    assert_eq!(aliases, vec!["dev", "prod", "staging"]);
  }

  #[test]
  fn directory_usage_sums_nested_file_sizes() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), vec![0u8; 10]).unwrap();
    let nested = dir.path().join("nested");
    std::fs::create_dir(&nested).unwrap();
    std::fs::write(nested.join("b.txt"), vec![0u8; 5]).unwrap();
    assert_eq!(directory_usage_bytes(dir.path()), 15);
  }

  #[test]
  fn clone_repo_local_checks_out_default_branch_with_origin() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    std::fs::create_dir(&source).unwrap();
    for args in [
      &["init", "-b", "main"][..],
      &[
        "-c",
        "user.name=T",
        "-c",
        "user.email=t@e.x",
        "commit",
        "--allow-empty",
        "-m",
        "init",
      ],
    ] {
      let status = Command::new("git")
        .current_dir(&source)
        .args(args)
        .status()
        .unwrap();
      assert!(status.success(), "git {args:?}");
    }
    std::fs::write(source.join("README.md"), "hi\n").unwrap();
    for args in [
      &["add", "README.md"][..],
      &[
        "-c",
        "user.name=T",
        "-c",
        "user.email=t@e.x",
        "commit",
        "-m",
        "readme",
      ],
    ] {
      let status = Command::new("git")
        .current_dir(&source)
        .args(args)
        .status()
        .unwrap();
      assert!(status.success(), "git {args:?}");
    }

    let destination = dir.path().join("clone");
    clone_repo_local(source.to_str().unwrap(), destination.to_str().unwrap()).unwrap();

    assert_eq!(
      std::fs::read_to_string(destination.join("README.md")).unwrap(),
      "hi\n"
    );
    let repo = gix::open(&destination).unwrap();
    assert_eq!(
      repo.head_name().unwrap().unwrap().shorten().to_string(),
      "main"
    );
    assert!(repo.find_remote("origin").is_ok());
  }

  #[test]
  fn clone_repo_local_fails_when_remote_head_branch_is_missing() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    std::fs::create_dir(&source).unwrap();
    for args in [
      &["init", "-b", "main"][..],
      &[
        "-c",
        "user.name=T",
        "-c",
        "user.email=t@e.x",
        "commit",
        "--allow-empty",
        "-m",
        "init",
      ],
      &["symbolic-ref", "HEAD", "refs/heads/missing"],
    ] {
      let status = Command::new("git")
        .current_dir(&source)
        .args(args)
        .status()
        .unwrap();
      assert!(status.success(), "git {args:?}");
    }

    let destination = dir.path().join("clone");
    let error =
      clone_repo_local(source.to_str().unwrap(), destination.to_str().unwrap()).unwrap_err();
    assert!(error.contains("missing"), "{error}");
    assert!(!destination.exists());
  }

  #[test]
  fn clone_repo_local_clones_an_empty_remote() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    std::fs::create_dir(&source).unwrap();
    let status = Command::new("git")
      .current_dir(&source)
      .args(["init", "-b", "main"])
      .status()
      .unwrap();
    assert!(status.success());

    let destination = dir.path().join("clone");
    clone_repo_local(source.to_str().unwrap(), destination.to_str().unwrap()).unwrap();
    assert!(gix::open(&destination).is_ok());
  }

  #[test]
  fn clone_repo_local_leaves_no_partial_destination_on_failure() {
    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("clone");
    let missing = dir.path().join("missing");
    let error =
      clone_repo_local(missing.to_str().unwrap(), destination.to_str().unwrap()).unwrap_err();
    assert!(error.starts_with("git_command_failed: "), "{error}");
    assert!(!destination.exists());
  }

  #[test]
  fn enforce_disk_quota_passes_under_the_limit() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), vec![0u8; 10]).unwrap();
    assert!(enforce_disk_quota_against(dir.path(), 100).is_ok());
  }

  #[test]
  fn enforce_disk_quota_returns_distinct_structured_error_over_the_limit() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), vec![0u8; 10]).unwrap();
    let error = enforce_disk_quota_against(dir.path(), 5).unwrap_err();
    assert!(
      error.starts_with("disk_quota_exceeded: "),
      "expected a distinct disk_quota_exceeded error, got: {error}"
    );
  }

  #[test]
  fn enforce_disk_quota_falls_back_to_parent_for_a_not_yet_created_path() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("existing.bin"), vec![0u8; 10]).unwrap();
    let not_yet_created = dir.path().join("new-repo");
    // The target path doesn't exist yet (this is `init_repo_path`'s case),
    // so usage is measured from its parent instead of failing the walk.
    let error = enforce_disk_quota_against(&not_yet_created, 5).unwrap_err();
    assert!(error.starts_with("disk_quota_exceeded: "));
    assert!(enforce_disk_quota_against(&not_yet_created, 100).is_ok());
  }

  #[test]
  fn quotes_remote_paths_with_single_quotes() {
    assert_eq!(shell_quote("/tmp/a b/it's"), "'/tmp/a b/it'\\''s'");
  }

  #[test]
  fn builds_ssh_shell_command_with_working_dir() {
    let (program, args) = build_ssh_shell_command("devbox", Some("/srv/my app"), None).unwrap();
    assert_eq!(program, "ssh");
    assert_eq!(args[..3], ["-t", "--", "devbox"]);
    assert!(args[3].contains("cd '/srv/my app'"));
  }

  #[test]
  fn rejects_unsafe_host_aliases() {
    for bad in [
      "dev; rm -rf /",
      "-oProxyCommand=touch /tmp/pwned",
      "-V",
      "dev\tbox",
      "dev'box",
    ] {
      assert!(build_ssh_shell_command(bad, None, None).is_err(), "{bad}");
    }
    for good in [
      "devbox",
      "me@dev-box.example.com",
      "dev_box:2222",
      "10.0.0.1",
    ] {
      assert!(build_ssh_shell_command(good, None, None).is_ok(), "{good}");
    }
  }

  #[test]
  fn builds_typed_remote_review_command_arguments() {
    assert_eq!(
      TreqCommandRequest::ListChanges {
        repo: "/srv/project".into(),
        workspace: Some("feature-auth".into()),
      }
      .cli_args()
      .unwrap(),
      vec![
        "changes",
        "list",
        "--repo",
        "/srv/project",
        "--workspace",
        "feature-auth",
        "--format",
        "json"
      ]
    );
  }

  #[test]
  fn rejects_invalid_paths_in_typed_requests() {
    let error = TreqCommandRequest::ReadFile {
      repo: "/srv/project".into(),
      workspace: Some("main".into()),
      path: "".into(),
      revision: FileRevision::WorkingCopy,
      start_line: None,
      end_line: None,
    }
    .cli_args()
    .unwrap_err();
    assert_eq!(error, "Remote file path is required");
  }

  #[test]
  fn expands_a_leading_tilde_to_the_home_folder() {
    let home = Some("/home/dev/");
    assert_eq!(expand_home("~", home), "/home/dev");
    assert_eq!(expand_home("~/src/app", home), "/home/dev/src/app");
    assert_eq!(expand_home("/srv/app", home), "/srv/app");
    assert_eq!(expand_home("~other/app", home), "~other/app");
    assert_eq!(expand_home("~/src/app", None), "~/src/app");
  }

  #[test]
  fn probe_reports_the_expanded_path() {
    let home = std::env::var("HOME").unwrap_or_default();
    let probe = probe_repo_path("~").unwrap();
    assert_eq!(probe.path, home.trim_end_matches('/'));
  }

  #[test]
  fn builds_workspace_summaries_arguments() {
    assert_eq!(
      TreqCommandRequest::WorkspaceSummaries { repo: "/r".into() }
        .cli_args()
        .unwrap(),
      vec!["workspace", "summaries", "--repo", "/r", "--format", "json"]
    );
  }

  #[test]
  fn builds_host_check_arguments() {
    assert_eq!(
      TreqCommandRequest::HostCheck { root: "~".into() }
        .cli_args()
        .unwrap(),
      vec!["repo", "host-check", "--repo", "~", "--format", "json"]
    );
    let check = host_check("/");
    assert!(check.root_exists);
    assert_eq!(check.treq_version, env!("CARGO_PKG_VERSION"));
  }

  #[test]
  fn builds_typed_probe_clone_init_arguments() {
    assert_eq!(
      TreqCommandRequest::ProbeRepo {
        repo: "/srv/project".into()
      }
      .cli_args()
      .unwrap(),
      vec![
        "repo",
        "probe",
        "--repo",
        "/srv/project",
        "--format",
        "json"
      ]
    );
    assert_eq!(
      TreqCommandRequest::CloneRepo {
        repo_url: "git@example.com:x/y.git".into(),
        destination: "/srv/project".into(),
        idempotency_key: "key-1".into(),
      }
      .cli_args()
      .unwrap(),
      vec![
        "repo",
        "clone",
        "--repo",
        "/srv/project",
        "--value",
        "git@example.com:x/y.git",
        "--idempotency-key",
        "key-1",
        "--format",
        "json"
      ]
    );
  }

  #[test]
  fn rejects_empty_idempotency_key() {
    let error = TreqCommandRequest::InitRepo {
      repo: "/srv/project".into(),
      idempotency_key: "  ".into(),
    }
    .cli_args()
    .unwrap_err();
    assert_eq!(error, "invalid_arguments: idempotency key is required");
  }

  // -- Idempotency ------------------------------------------------------------

  fn test_request(repo: &str) -> TreqCommandRequest {
    TreqCommandRequest::CreateWorkspace {
      repo: repo.to_string(),
      branch_name: "feature-x".into(),
      source_branch: None,
      metadata: None,
      idempotency_key: String::new(),
    }
  }

  #[test]
  fn idempotency_key_replays_cached_result_for_a_create_style_mutation_instead_of_rerunning() {
    let dir = tempfile::TempDir::new().unwrap();
    let repo = dir.path().to_str().unwrap();
    let request = test_request(repo);
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let calls_clone = calls.clone();
    let run = move || {
      let n = calls_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
      Ok(serde_json::json!({ "created_count": n }))
    };
    let key = format!("test-create-{}", uuid_like());
    let first =
      with_idempotency_key(repo, "test.create", Some(&key), &request, run.clone()).unwrap();
    let second = with_idempotency_key(repo, "test.create", Some(&key), &request, run).unwrap();
    // The second call must replay the first response rather than invoking
    // `run` again — a retried "create" mutation must not duplicate work.
    assert_eq!(first, second);
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
  }

  #[test]
  fn missing_idempotency_key_always_reruns_the_operation() {
    let dir = tempfile::TempDir::new().unwrap();
    let repo = dir.path().to_str().unwrap();
    let request = test_request(repo);
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    for _ in 0..2 {
      let calls_clone = calls.clone();
      with_idempotency_key(repo, "test.no-key", None, &request, move || {
        calls_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(serde_json::json!(null))
      })
      .unwrap();
    }
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
  }

  #[test]
  fn reusing_key_for_a_different_request_is_a_structured_conflict() {
    let dir = tempfile::TempDir::new().unwrap();
    let repo = dir.path().to_str().unwrap();
    let key = format!("test-conflict-{}", uuid_like());
    let first_request = test_request(repo);
    with_idempotency_key(repo, "test.create", Some(&key), &first_request, || {
      Ok(serde_json::json!({ "id": 1 }))
    })
    .unwrap();

    let second_request = TreqCommandRequest::CreateWorkspace {
      repo: repo.to_string(),
      branch_name: "different-branch".into(),
      source_branch: None,
      metadata: None,
      idempotency_key: String::new(),
    };
    let error = with_idempotency_key(repo, "test.create", Some(&key), &second_request, || {
      panic!("must not execute a mutation reusing a key for a different request")
    })
    .unwrap_err();
    assert!(error.starts_with("idempotency_conflict:"), "{error}");
  }

  #[test]
  fn a_failed_mutation_frees_its_idempotency_key_for_reuse() {
    let dir = tempfile::TempDir::new().unwrap();
    let repo = dir.path().to_str().unwrap();
    let request = test_request(repo);
    let key = format!("test-fail-{}", uuid_like());

    let error = with_idempotency_key(repo, "test.create", Some(&key), &request, || {
      Err("workspace_not_found: boom".to_string())
    })
    .unwrap_err();
    assert_eq!(error, "workspace_not_found: boom");

    // The failed attempt must not leave a pending claim behind: a retry
    // with the same key runs the mutation again rather than replaying or
    // conflicting.
    let result = with_idempotency_key(repo, "test.create", Some(&key), &request, || {
      Ok(serde_json::json!({ "id": 2 }))
    })
    .unwrap();
    assert_eq!(result, serde_json::json!({ "id": 2 }));
  }

  /// `describe_commit`'s CLI dispatch takes no idempotency key at all: it is
  /// naturally idempotent (rewriting a description is safe to repeat), so
  /// the typed request has no field for one and every call always runs.
  #[test]
  fn describe_commit_request_has_no_idempotency_key_field() {
    let args = TreqCommandRequest::DescribeCommit {
      repo: "/srv/project".into(),
      workspace: "1".into(),
      commit: "abc123".into(),
      message: "new description".into(),
    }
    .cli_args()
    .unwrap();
    assert!(!args.iter().any(|a| a == "--idempotency-key"));
  }

  // -- Change propagation across concurrent clients ---------------------------

  #[test]
  fn builds_typed_change_marker_arguments() {
    assert_eq!(
      TreqCommandRequest::WorkspaceChangeMarker {
        repo: "/srv/project".into(),
        workspace: Some("feature-auth".into()),
      }
      .cli_args()
      .unwrap(),
      vec![
        "workspace",
        "marker",
        "--repo",
        "/srv/project",
        "--workspace",
        "feature-auth",
        "--format",
        "json"
      ]
    );
    // Marker is also valid for the repo root (no workspace scope).
    assert_eq!(
      TreqCommandRequest::WorkspaceChangeMarker {
        repo: "/srv/project".into(),
        workspace: None,
      }
      .cli_args()
      .unwrap(),
      vec![
        "workspace",
        "marker",
        "--repo",
        "/srv/project",
        "--format",
        "json"
      ]
    );
  }

  #[test]
  fn init_repo_creates_a_colocated_repository() {
    let repo_dir = tempfile::tempdir().unwrap();
    let repo_path = repo_dir.path().join("new-repo");
    let inspection = execute_local_request(TreqCommandRequest::InitRepo {
      repo: repo_path.to_str().unwrap().to_string(),
      idempotency_key: "init-colocated".into(),
    })
    .unwrap();
    assert_eq!(
      inspection["repository_type"], "jj_colocated",
      "{inspection}"
    );
  }

  #[test]
  fn init_repo_colocates_jj_with_a_plain_git_repository() {
    let repo_dir = tempfile::tempdir().unwrap();
    let repo_path = repo_dir.path().to_str().unwrap().to_string();
    let git = |args: &[&str]| {
      let status = std::process::Command::new("git")
        .args(["-c", "user.name=T", "-c", "user.email=t@example.com"])
        .args(args)
        .current_dir(&repo_path)
        .status()
        .unwrap();
      assert!(status.success(), "git {args:?}");
    };
    git(&["init", "-q", "-b", "main"]);
    fs::write(repo_dir.path().join("README.md"), "# r\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "Initial commit"]);
    let inspection = execute_local_request(TreqCommandRequest::InitRepo {
      repo: repo_path.clone(),
      idempotency_key: "init-git".into(),
    })
    .unwrap();
    assert_eq!(
      inspection["repository_type"], "jj_colocated",
      "{inspection}"
    );
    assert_eq!(inspection["default_branch"], "main", "{inspection}");
  }

  #[test]
  fn rename_workspace_rejection_is_an_error() {
    let repo_dir = tempfile::tempdir().unwrap();
    let repo_path = repo_dir.path().to_str().unwrap().to_string();
    execute_local_request(TreqCommandRequest::InitRepo {
      repo: repo_path.clone(),
      idempotency_key: "init-rename".into(),
    })
    .unwrap();
    let id = crate::local_db::add_workspace(
      &repo_path,
      "feat-a".into(),
      "feat-a".into(),
      "feat/a".into(),
      None,
      None,
      None,
    )
    .unwrap();

    let error = execute_local_request(TreqCommandRequest::RenameWorkspace {
      repo: repo_path,
      workspace: id.to_string(),
      new_name: "feat/a".into(),
      idempotency_key: "rename-same".into(),
    })
    .unwrap_err();

    assert!(error.starts_with("invalid_arguments: "), "{error}");
  }

  #[test]
  fn change_marker_reflects_new_operations_and_local_dispatch_matches_direct_jj_call() {
    let repo_dir = tempfile::tempdir().unwrap();
    let repo_path = repo_dir.path().to_str().unwrap().to_string();
    crate::core::remote::execute_local_request(TreqCommandRequest::InitRepo {
      repo: repo_path.clone(),
      idempotency_key: "init-1".into(),
    })
    .unwrap();

    let before = execute_local_request(TreqCommandRequest::WorkspaceChangeMarker {
      repo: repo_path.clone(),
      workspace: None,
    })
    .unwrap();
    let before: WorkspaceChangeMarker = serde_json::from_value(before).unwrap();
    assert_eq!(
      before.operation_id,
      crate::jj::jj_head_operation_id(&repo_path).unwrap()
    );

    // A mutation (a new commit) must advance the operation log, so the
    // marker a second client polls for changes even though it never
    // initiated the mutation itself.
    std::fs::write(repo_dir.path().join("a.txt"), "a\n").unwrap();
    execute_local_request(TreqCommandRequest::CreateCommit {
      repo: repo_path.clone(),
      workspace: None,
      message: "test commit".into(),
      base_change_id: None,
      idempotency_key: "commit-1".into(),
    })
    .unwrap();

    let after = execute_local_request(TreqCommandRequest::WorkspaceChangeMarker {
      repo: repo_path,
      workspace: None,
    })
    .unwrap();
    let after: WorkspaceChangeMarker = serde_json::from_value(after).unwrap();

    assert_ne!(
      before.operation_id, after.operation_id,
      "operation id must change after a mutation so a foreign client can detect it"
    );
  }

  fn create_commit_in_fresh_repo(message: &str) -> Result<serde_json::Value, String> {
    let repo_dir = tempfile::tempdir().unwrap();
    let repo = repo_dir.path().to_str().unwrap().to_string();
    execute_local_request(TreqCommandRequest::InitRepo {
      repo: repo.clone(),
      idempotency_key: "init-1".into(),
    })
    .unwrap();
    if message != "nothing" {
      std::fs::write(repo_dir.path().join("a.txt"), "a\n").unwrap();
    }
    execute_local_request(TreqCommandRequest::CreateCommit {
      repo,
      workspace: None,
      message: message.into(),
      base_change_id: None,
      idempotency_key: "commit-1".into(),
    })
  }

  #[test]
  fn create_commit_refuses_when_there_is_nothing_to_commit() {
    let error = create_commit_in_fresh_repo("nothing").unwrap_err();
    assert!(
      error.starts_with("invalid_arguments:") && error.contains("nothing to commit"),
      "{error}"
    );
  }

  #[test]
  fn create_commit_rejects_whitespace_only_message() {
    let error = create_commit_in_fresh_repo("   ").unwrap_err();
    assert!(
      error.starts_with("invalid_arguments:") && error.contains("message"),
      "{error}"
    );
  }

  #[test]
  fn concurrent_creates_commit_one_dirty_working_copy_once() {
    let repo_dir = tempfile::tempdir().unwrap();
    let repo = repo_dir.path().to_str().unwrap().to_string();
    execute_local_request(TreqCommandRequest::InitRepo {
      repo: repo.clone(),
      idempotency_key: "init-1".into(),
    })
    .unwrap();
    std::fs::write(repo_dir.path().join("a.txt"), "a\n").unwrap();

    let start = std::sync::Barrier::new(5);
    let results: Vec<_> = std::thread::scope(|scope| {
      let handles: Vec<_> = (0..5)
        .map(|i| {
          let (repo, start) = (repo.clone(), &start);
          scope.spawn(move || {
            start.wait();
            execute_local_request(TreqCommandRequest::CreateCommit {
              repo,
              workspace: None,
              message: format!("parallel {i}"),
              base_change_id: None,
              idempotency_key: format!("par-{i}"),
            })
          })
        })
        .collect();
      handles.into_iter().map(|h| h.join().unwrap()).collect()
    });

    assert_eq!(
      results.iter().filter(|r| r.is_ok()).count(),
      1,
      "{results:?}"
    );
    for error in results.iter().filter_map(|r| r.as_ref().err()) {
      assert!(error.contains("nothing to commit"), "{error}");
    }
  }

  #[test]
  fn change_marker_detects_out_of_band_file_writes_and_stays_stable_when_idle() {
    let repo_dir = tempfile::tempdir().unwrap();
    let repo_path = repo_dir.path().to_str().unwrap().to_string();
    execute_local_request(TreqCommandRequest::InitRepo {
      repo: repo_path.clone(),
      idempotency_key: "init-1".into(),
    })
    .unwrap();
    let marker = || -> String {
      let value = execute_local_request(TreqCommandRequest::WorkspaceChangeMarker {
        repo: repo_path.clone(),
        workspace: None,
      })
      .unwrap();
      serde_json::from_value::<WorkspaceChangeMarker>(value)
        .unwrap()
        .operation_id
    };

    let first = marker();
    let second = marker();
    assert_eq!(first, second, "an idle repo must return a stable marker");
    assert_eq!(
      crate::jj::jj_head_operation_id(&repo_path).unwrap(),
      first,
      "polling an idle repo must not add operations"
    );

    // A terminal or agent writing a file never runs jj, so only the snapshot
    // taken while reading the marker can surface it.
    std::fs::write(repo_dir.path().join("agent-output.txt"), "edited\n").unwrap();
    let after_write = marker();
    assert_ne!(first, after_write);
    assert_eq!(marker(), after_write);
  }

  fn uuid_like() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    format!(
      "{}-{}",
      std::process::id(),
      SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos()
    )
  }

  // -- Retrying after network loss: classification and verification -----------

  #[test]
  fn reads_are_never_classified_as_mutations() {
    let reads = [
      TreqCommandRequest::InspectRepository { repo: "/r".into() },
      TreqCommandRequest::RepositoryStatus { repo: "/r".into() },
      TreqCommandRequest::ListBranches { repo: "/r".into() },
      TreqCommandRequest::ListWorkspaces { repo: "/r".into() },
      TreqCommandRequest::ListChanges {
        repo: "/r".into(),
        workspace: None,
      },
      TreqCommandRequest::ListCommits {
        repo: "/r".into(),
        workspace: None,
      },
      TreqCommandRequest::ListConflicts {
        repo: "/r".into(),
        workspace: None,
      },
      TreqCommandRequest::ProbeRepo { repo: "/r".into() },
      TreqCommandRequest::AgentStatus {
        repo: "/r".into(),
        workspace: "1".into(),
      },
    ];
    for request in reads {
      assert!(
        !request.is_mutation(),
        "expected a read-only request to not be classified as a mutation: {request:?}"
      );
    }
  }

  #[test]
  fn workspace_commit_conflict_git_and_agent_mutations_are_all_classified_as_mutations() {
    let mutations = [
      TreqCommandRequest::CreateWorkspace {
        repo: "/r".into(),
        branch_name: "b".into(),
        source_branch: None,
        metadata: None,
        idempotency_key: "k".into(),
      },
      TreqCommandRequest::DeleteWorkspace {
        repo: "/r".into(),
        workspace: "1".into(),
      },
      TreqCommandRequest::RebaseWorkspace {
        repo: "/r".into(),
        workspace: "1".into(),
        target_branch: "main".into(),
        idempotency_key: "k".into(),
      },
      TreqCommandRequest::CreateCommit {
        repo: "/r".into(),
        workspace: None,
        message: "m".into(),
        base_change_id: None,
        idempotency_key: "k".into(),
      },
      TreqCommandRequest::AbandonCommit {
        repo: "/r".into(),
        workspace: "1".into(),
        commit: "c".into(),
        idempotency_key: "k".into(),
      },
      TreqCommandRequest::ResolveConflict {
        repo: "/r".into(),
        revision: "c".into(),
        sides: vec![],
        idempotency_key: "k".into(),
      },
      TreqCommandRequest::GitFetch { repo: "/r".into() },
      TreqCommandRequest::GitPush {
        repo: "/r".into(),
        workspace: None,
        idempotency_key: "k".into(),
      },
      TreqCommandRequest::AgentStart {
        repo: "/r".into(),
        workspace: "1".into(),
        agent: "claude".into(),
        prompt: "p".into(),
        idempotency_key: "k".into(),
      },
      TreqCommandRequest::AgentStop {
        repo: "/r".into(),
        workspace: "1".into(),
      },
    ];
    for request in mutations {
      assert!(
        request.is_mutation(),
        "expected a mutating request to be classified as a mutation: {request:?}"
      );
    }
  }

  #[test]
  fn create_workspace_verification_detects_already_applied_and_not_applied() {
    let verification = verification_for(&TreqCommandRequest::CreateWorkspace {
      repo: "/r".into(),
      branch_name: "feature-x".into(),
      source_branch: None,
      metadata: None,
      idempotency_key: "key".into(),
    })
    .expect("CreateWorkspace has a verification recipe");
    assert!(matches!(
      verification.read_request,
      TreqCommandRequest::ListWorkspaces { .. }
    ));

    let applied = serde_json::json!([{ "id": 1, "branch_name": "feature-x" }]);
    assert_eq!(
      (verification.check)(&applied),
      MutationVerificationOutcome::AlreadyApplied
    );

    let not_applied = serde_json::json!([{ "id": 1, "branch_name": "some-other-branch" }]);
    assert_eq!(
      (verification.check)(&not_applied),
      MutationVerificationOutcome::NotApplied
    );

    let unexpected_shape = serde_json::json!({ "not": "an array" });
    assert_eq!(
      (verification.check)(&unexpected_shape),
      MutationVerificationOutcome::Ambiguous
    );
  }

  #[test]
  fn delete_workspace_verification_treats_absence_as_applied() {
    let verification = verification_for(&TreqCommandRequest::DeleteWorkspace {
      repo: "/r".into(),
      workspace: "42".into(),
    })
    .expect("DeleteWorkspace has a verification recipe");

    let still_present = serde_json::json!([{ "id": "42" }]);
    assert_eq!(
      (verification.check)(&still_present),
      MutationVerificationOutcome::NotApplied
    );

    let gone = serde_json::json!([{ "id": "7" }]);
    assert_eq!(
      (verification.check)(&gone),
      MutationVerificationOutcome::AlreadyApplied
    );

    // `ListWorkspaces` serializes ids as numbers.
    let still_present_numeric = serde_json::json!([{ "id": 42 }]);
    assert_eq!(
      (verification.check)(&still_present_numeric),
      MutationVerificationOutcome::NotApplied
    );
  }

  #[test]
  fn archive_workspace_verification_treats_absence_as_applied() {
    let verification = verification_for(&TreqCommandRequest::ArchiveWorkspace {
      repo: "/r".into(),
      workspace: "42".into(),
    })
    .expect("ArchiveWorkspace has a verification recipe");
    let check = verification.check;
    let present = serde_json::json!([{ "id": 42 }]);
    assert_eq!(check(&present), MutationVerificationOutcome::NotApplied);
    let gone = serde_json::json!([{ "id": 7 }]);
    assert_eq!(check(&gone), MutationVerificationOutcome::AlreadyApplied);
  }

  #[test]
  fn switch_repo_branch_verification_reads_the_current_branch() {
    let check = verification_for(&TreqCommandRequest::SwitchRepoBranch {
      repo: "/r".into(),
      bookmark: "feat".into(),
    })
    .expect("SwitchRepoBranch has a verification recipe")
    .check;
    let on = |branch: serde_json::Value| check(&serde_json::json!({ "current_branch": branch }));
    assert_eq!(
      on("feat".into()),
      MutationVerificationOutcome::AlreadyApplied
    );
    assert_eq!(on("main".into()), MutationVerificationOutcome::NotApplied);
    assert_eq!(
      on(serde_json::Value::Null),
      MutationVerificationOutcome::NotApplied
    );
    assert_eq!(
      check(&serde_json::json!({})),
      MutationVerificationOutcome::Ambiguous
    );
  }

  #[test]
  fn rename_workspace_verification_reads_the_current_branch() {
    let verification = verification_for(&TreqCommandRequest::RenameWorkspace {
      repo: "/r".into(),
      workspace: "1".into(),
      new_name: "feat-new".into(),
      idempotency_key: "k".into(),
    })
    .unwrap();
    let renamed = serde_json::json!({ "current": { "branch_name": "feat-new" } });
    assert_eq!(
      (verification.check)(&renamed),
      MutationVerificationOutcome::AlreadyApplied
    );
    let unchanged = serde_json::json!({ "current": { "branch_name": "feat-old" } });
    assert_eq!(
      (verification.check)(&unchanged),
      MutationVerificationOutcome::NotApplied
    );
  }

  #[test]
  fn agent_start_verification_reads_the_running_flag() {
    let verification = verification_for(&TreqCommandRequest::AgentStart {
      repo: "/r".into(),
      workspace: "1".into(),
      agent: "claude".into(),
      prompt: "p".into(),
      idempotency_key: "k".into(),
    })
    .expect("AgentStart has a verification recipe");
    assert!(matches!(
      verification.read_request,
      TreqCommandRequest::AgentStatus { .. }
    ));

    assert_eq!(
      (verification.check)(&serde_json::json!({ "running": true })),
      MutationVerificationOutcome::AlreadyApplied
    );
    assert_eq!(
      (verification.check)(&serde_json::json!({ "running": false })),
      MutationVerificationOutcome::NotApplied
    );
    assert_eq!(
      (verification.check)(&serde_json::json!({})),
      MutationVerificationOutcome::Ambiguous
    );
  }

  #[test]
  fn undo_commit_verification_treats_a_missing_change_as_applied() {
    let check = verification_for(&TreqCommandRequest::UndoCommit {
      repo: "/r".into(),
      workspace: "1".into(),
      commit: "abc".into(),
    })
    .expect("UndoCommit has a verification recipe")
    .check;
    let present = serde_json::json!({ "commits": [{ "change_id": "abc" }] });
    assert_eq!(check(&present), MutationVerificationOutcome::NotApplied);
    let gone = serde_json::json!({ "commits": [{ "change_id": "xyz" }] });
    assert_eq!(check(&gone), MutationVerificationOutcome::AlreadyApplied);
  }

  #[test]
  fn restore_file_has_no_verification_recipe() {
    // Documented carve-out: rather than guess at a false-positive check, a
    // mutation with no reliable observable-state read falls back to
    // `MutationVerificationOutcome::Ambiguous` in `retry_after_reconnect`.
    let request = TreqCommandRequest::RestoreFile {
      repo: "/r".into(),
      workspace: None,
      path: "a.txt".into(),
    };
    assert!(verification_for(&request).is_none());
  }

  #[test]
  fn resolve_conflict_verifies_against_the_revision_conflict_flag() {
    let request = TreqCommandRequest::ResolveConflict {
      repo: "/r".into(),
      revision: "qpvuntsm".into(),
      sides: vec!["1".into()],
      idempotency_key: "k".into(),
    };
    let verification = verification_for(&request).expect("ResolveConflict has a recipe");
    assert!(matches!(
      verification.read_request,
      TreqCommandRequest::ListCommits {
        workspace: None,
        ..
      }
    ));
    let log = |has_conflicts: bool| {
      serde_json::json!({
        "commits": [
          { "change_id": "zzzzzzzz", "has_conflicts": true },
          { "change_id": "qpvuntsmwlqt", "has_conflicts": has_conflicts },
        ]
      })
    };
    assert_eq!(
      (verification.check)(&log(false)),
      MutationVerificationOutcome::AlreadyApplied
    );
    assert_eq!(
      (verification.check)(&log(true)),
      MutationVerificationOutcome::NotApplied
    );
    // The old recipe matched change ids against `ListConflicts` file paths.
    assert_eq!(
      (verification.check)(&serde_json::json!(["src/a.rs"])),
      MutationVerificationOutcome::Ambiguous
    );
    assert_eq!(
      (verification.check)(&serde_json::json!({ "commits": [] })),
      MutationVerificationOutcome::Ambiguous
    );
  }

  #[test]
  fn restore_all_verification_never_reruns_over_remaining_changes() {
    let request = TreqCommandRequest::RestoreAll {
      repo: "/r".into(),
      workspace: Some("1".into()),
    };
    let check = verification_for(&request).expect("recipe").check;
    assert_eq!(
      check(&serde_json::json!([])),
      MutationVerificationOutcome::AlreadyApplied
    );
    // The discard landed but its reply was lost, and an agent wrote a new
    // file before reconnect verification. `NotApplied` would make
    // `retry_after_reconnect` resend RestoreAll and delete that edit.
    let edited_after_discard = serde_json::json!([{ "path": "agent-new.txt" }]);
    assert_eq!(
      check(&edited_after_discard),
      MutationVerificationOutcome::Ambiguous
    );
    assert_eq!(
      check(&serde_json::json!({})),
      MutationVerificationOutcome::Ambiguous
    );
  }

  #[test]
  fn split_commit_rejects_empty_files_and_hunks() {
    let error = TreqCommandRequest::SplitCommit {
      repo: "/srv/project".into(),
      workspace: "1".into(),
      commit: "abc".into(),
      files: vec![],
      hunks: vec![],
      message: String::new(),
      idempotency_key: "split-1".into(),
    }
    .cli_args()
    .unwrap_err();
    assert!(error.contains("files or hunks"));
  }

  #[test]
  fn split_commit_encodes_selected_files_and_hunks() {
    let args = TreqCommandRequest::SplitCommit {
      repo: "/srv/project".into(),
      workspace: "1".into(),
      commit: "abc".into(),
      files: vec!["a.rs".into()],
      hunks: vec![crate::core::workspaces::HunkSpec {
        file_path: "b.rs".into(),
        start_line: 2,
        end_line: 4,
      }],
      message: String::new(),
      idempotency_key: "split-1".into(),
    }
    .cli_args()
    .unwrap();
    assert!(args.contains(&"commits".into()));
    assert!(args.contains(&"split".into()));
    assert!(args.contains(&"a.rs".into()));
    assert!(args.iter().any(|a| a.contains("b.rs:2-4")));
    assert!(args.contains(&"--idempotency-key".into()));
  }

  #[test]
  fn split_commit_execute_does_not_return_not_implemented() {
    let error = execute_local_request(TreqCommandRequest::SplitCommit {
      repo: "/does/not/exist".into(),
      workspace: "1".into(),
      commit: "abc".into(),
      files: vec!["a.rs".into()],
      hunks: vec![],
      message: String::new(),
      idempotency_key: "split-exec".into(),
    })
    .unwrap_err();
    assert!(
      !error.contains("not_implemented"),
      "split must be a real typed mutation, got: {error}"
    );
  }

  #[test]
  fn describe_commit_does_not_require_an_idempotency_key() {
    let request = TreqCommandRequest::DescribeCommit {
      repo: "/r".into(),
      workspace: "1".into(),
      commit: "c".into(),
      message: "m".into(),
    };
    assert!(!request.requires_idempotency_key());
    assert!(request
      .cli_args()
      .unwrap()
      .iter()
      .all(|a| a != "--idempotency-key"));
  }

  #[test]
  fn create_commit_requires_a_non_empty_idempotency_key() {
    let request = TreqCommandRequest::CreateCommit {
      repo: "/r".into(),
      workspace: None,
      message: "m".into(),
      base_change_id: None,
      idempotency_key: " ".into(),
    };
    assert!(request.requires_idempotency_key());
    assert!(request
      .cli_args()
      .unwrap_err()
      .contains("idempotency key is required"));
  }

  fn seed_repo_with_workspaces(
    root: &Path,
    owner: &str,
    name: &str,
    workspaces: &[&str],
  ) -> String {
    let repo = root.join(owner).join(name);
    fs::create_dir_all(repo.join(".git")).unwrap();
    let repo_path = repo.to_str().unwrap().to_string();
    for workspace in workspaces {
      crate::local_db::add_workspace(
        &repo_path,
        workspace.to_string(),
        format!("{repo_path}/.treq/workspaces/{workspace}"),
        format!("feat/{workspace}"),
        None,
        None,
        None,
      )
      .unwrap();
    }
    repo_path
  }

  #[test]
  fn machine_usage_counts_repositories_workspaces_and_bytes_under_root() {
    let root = tempfile::tempdir().unwrap();
    seed_repo_with_workspaces(root.path(), "acme", "api", &["one", "two"]);
    let web = seed_repo_with_workspaces(root.path(), "acme", "web", &["three"]);
    fs::write(Path::new(&web).join("README.md"), vec![b'x'; 4096]).unwrap();
    // A plain directory that is not a repository is not counted.
    fs::create_dir_all(root.path().join("acme").join("notes")).unwrap();

    let value = execute_local_request(TreqCommandRequest::MachineUsage {
      root: root.path().to_str().unwrap().to_string(),
    })
    .unwrap();
    let usage: MachineUsageReport = serde_json::from_value(value).unwrap();

    assert_eq!(usage.repository_count, 2);
    assert_eq!(usage.workspace_count, 3);
    assert!(usage.disk_used_bytes >= 4096);
  }

  #[test]
  fn machine_usage_returns_zero_when_root_is_missing() {
    let root = tempfile::tempdir().unwrap();
    let missing = root.path().join("repos");

    let value = execute_local_request(TreqCommandRequest::MachineUsage {
      root: missing.to_str().unwrap().to_string(),
    })
    .unwrap();
    let usage: MachineUsageReport = serde_json::from_value(value).unwrap();

    assert_eq!(usage, MachineUsageReport::default());
  }

  #[test]
  fn machine_usage_is_a_read_only_repo_cli_command() {
    let request = TreqCommandRequest::MachineUsage {
      root: "/home/sprite/repos".into(),
    };

    assert!(!request.is_mutation());
    assert_eq!(
      request.cli_args().unwrap(),
      vec![
        "repo",
        "usage",
        "--repo",
        "/home/sprite/repos",
        "--format",
        "json"
      ]
    );
  }

  #[test]
  fn a_host_holds_terminals_by_its_backend_and_older_hosts_by_their_multiplexer() {
    use crate::core::pty_remote_supervisor::PtyBackend;
    let check = |terminal: Option<&str>, tmux: bool| RemoteHostCheck {
      treq_version: "0.3.0".into(),
      root: "/".into(),
      root_exists: true,
      terminal: terminal.map(str::to_string),
      tmux,
      agents: vec![],
    };
    assert!(check(Some("daemon"), false).holds_terminals(PtyBackend::Daemon));
    assert!(!check(Some("daemon"), false).holds_terminals(PtyBackend::Tmux));
    assert!(check(Some("daemon"), true).holds_terminals(PtyBackend::Tmux));
    assert!(check(Some("tmux"), false).holds_terminals(PtyBackend::Tmux));
    assert!(!check(None, false).holds_terminals(PtyBackend::Daemon));
    // A host check from before the field existed.
    let old: RemoteHostCheck = serde_json::from_value(serde_json::json!({
      "treq_version": "0.2.0", "root": "/", "root_exists": true,
      "terminal": "tmux", "agents": []
    }))
    .unwrap();
    assert!(!old.tmux);
    assert!(old.holds_terminals(PtyBackend::Daemon));
  }

  #[test]
  fn this_host_reports_the_daemon() {
    let report = host_check("/");
    assert_eq!(report.terminal.as_deref(), Some("daemon"));
    assert_eq!(
      report.tmux,
      crate::core::pty_remote_supervisor::tmux_available()
    );
  }

  #[test]
  fn pty_requests_name_tmux_only_when_the_machine_uses_it() {
    use crate::core::pty_remote_supervisor::PtyBackend;
    let list = |backend| TreqCommandRequest::PtyList {
      backend,
      repo: "/r".into(),
      workspace: None,
    };
    let daemon = list(PtyBackend::Daemon).cli_args().unwrap();
    assert!(!daemon.contains(&"--backend".to_string()), "{daemon:?}");
    let tmux = list(PtyBackend::Tmux).cli_args().unwrap();
    let at = tmux.iter().position(|a| a == "--backend").unwrap();
    assert_eq!(tmux[at + 1], "tmux");
    assert_eq!(
      TreqCommandRequest::PtyStatus.cli_args().unwrap()[..2],
      ["pty-remote".to_string(), "status".to_string()]
    );
    assert_eq!(
      TreqCommandRequest::PtyReset {
        backend: PtyBackend::Daemon
      }
      .cli_args()
      .unwrap()[..2],
      ["pty-remote".to_string(), "reset".to_string()]
    );
  }

  #[test]
  fn kind_names_match_serde_kind_tags() {
    let sample = TreqCommandRequest::GitFetch { repo: "/r".into() };
    let value = serde_json::to_value(&sample).unwrap();
    assert_eq!(value["kind"], "GitFetch");
    assert!(TreqCommandRequest::KIND_NAMES.contains(&sample.kind_name()));
    assert_eq!(TreqCommandRequest::KIND_NAMES.len(), 77);
  }

  #[test]
  fn rejects_unknown_command_kind_at_json_boundary() {
    let parsed = serde_json::from_value::<TreqCommandRequest>(serde_json::json!({
      "kind": "RunArbitraryShell",
      "cmd": "rm -rf /"
    }));
    assert!(parsed.is_err());
  }

  // -- CreateCommit verification uses a pre-commit marker -----------------------

  fn create_commit(base: Option<&str>) -> TreqCommandRequest {
    TreqCommandRequest::CreateCommit {
      repo: "/r".into(),
      workspace: Some("1".into()),
      message: "wip".into(),
      base_change_id: base.map(str::to_string),
      idempotency_key: "k".into(),
    }
  }

  fn log_commit(change_id: &str, description: &str, is_working_copy: bool) -> serde_json::Value {
    serde_json::json!({
      "change_id": change_id,
      "commit_id": format!("{change_id}-commit"),
      "description": description,
      "is_working_copy": is_working_copy,
    })
  }

  #[test]
  fn create_commit_without_a_baseline_has_no_verification_recipe() {
    // Matching on the message alone is a false positive for repeated
    // messages, so without a marker the caller must report ambiguity.
    assert!(verification_for(&create_commit(None)).is_none());
  }

  #[test]
  fn create_commit_verification_ignores_older_commits_with_the_same_message() {
    let verification = verification_for(&create_commit(Some("base00000000"))).unwrap();
    let state = serde_json::json!({
      "commits": [
        log_commit("base00000000", "", true),
        log_commit("older0000000", "wip", false),
      ],
    });
    assert_eq!(
      (verification.check)(&state),
      MutationVerificationOutcome::NotApplied
    );
  }

  #[test]
  fn create_commit_verification_detects_the_committed_working_copy() {
    let verification = verification_for(&create_commit(Some("base00000000"))).unwrap();
    let state = serde_json::json!({
      "commits": [
        log_commit("newwc0000000", "", true),
        log_commit("base00000000", "wip\n", false),
      ],
    });
    assert_eq!(
      (verification.check)(&state),
      MutationVerificationOutcome::AlreadyApplied
    );
  }

  #[test]
  fn create_commit_verification_reads_the_tentative_working_copy() {
    let verification = verification_for(&create_commit(Some("base00000000"))).unwrap();
    let state = serde_json::json!({
      "commits": [log_commit("older0000000", "wip", false)],
      "tentative_working_copy": {
        "workspace_label": "ws",
        "commit": log_commit("base00000000", "", true),
      },
    });
    assert_eq!(
      (verification.check)(&state),
      MutationVerificationOutcome::NotApplied
    );
  }

  #[test]
  fn create_commit_verification_is_ambiguous_when_the_marker_is_missing_or_rewritten() {
    let verification = verification_for(&create_commit(Some("base00000000"))).unwrap();
    let missing = serde_json::json!({ "commits": [log_commit("older0000000", "wip", false)] });
    assert_eq!(
      (verification.check)(&missing),
      MutationVerificationOutcome::Ambiguous
    );
    let rewritten = serde_json::json!({ "commits": [log_commit("base00000000", "other", false)] });
    assert_eq!(
      (verification.check)(&rewritten),
      MutationVerificationOutcome::Ambiguous
    );
  }

  #[test]
  fn baseline_is_read_only_for_create_commit_without_a_marker() {
    assert!(matches!(
      baseline_request(&create_commit(None)),
      Some(TreqCommandRequest::WorkspaceChangeMarker { workspace: Some(ref w), .. }) if w == "1"
    ));
    assert!(baseline_request(&create_commit(Some("base00000000"))).is_none());
    assert!(baseline_request(&TreqCommandRequest::GitFetch { repo: "/r".into() }).is_none());

    let filled = with_verification_baseline(
      create_commit(None),
      WorkspaceChangeMarker {
        operation_id: "op".into(),
        working_copy_change_id: Some("base00000000".into()),
      },
    );
    assert_eq!(filled, create_commit(Some("base00000000")));
  }

  #[test]
  fn create_commit_json_without_an_idempotency_key_is_rejected() {
    // The desktop client must always send a key: the field is required at the
    // IPC boundary, so a request without it never reaches the remote.
    let parsed = serde_json::from_value::<TreqCommandRequest>(serde_json::json!({
      "kind": "CreateCommit",
      "repo": "/r",
      "workspace": null,
      "message": "m",
    }));
    assert!(parsed.is_err());
  }

  #[test]
  fn create_commit_carries_its_marker_on_the_cli() {
    let args = create_commit(Some("base00000000")).cli_args().unwrap();
    let target = args.iter().position(|a| a == "--target").unwrap();
    assert_eq!(args[target + 1], "base00000000");
  }

  // -- Split and move fidelity --------------------------------------------------

  #[test]
  fn split_commit_refuses_hunk_selections_instead_of_widening_to_whole_files() {
    let error = execute_local_request(TreqCommandRequest::SplitCommit {
      repo: "/does/not/exist".into(),
      workspace: "1".into(),
      commit: "@".into(),
      files: vec![],
      hunks: vec![crate::core::workspaces::HunkSpec {
        file_path: "a.rs".into(),
        start_line: 1,
        end_line: 2,
      }],
      message: "first".into(),
      idempotency_key: format!("split-hunks-{}", uuid_like()),
    })
    .unwrap_err();
    assert!(error.starts_with("unsupported:"), "got: {error}");
  }

  #[test]
  fn split_commit_sends_its_message_on_the_cli() {
    let args = TreqCommandRequest::SplitCommit {
      repo: "/srv/project".into(),
      workspace: "1".into(),
      commit: "@".into(),
      files: vec!["a.rs".into()],
      hunks: vec![],
      message: "first half".into(),
      idempotency_key: "split-1".into(),
    }
    .cli_args()
    .unwrap();
    let message = args.iter().position(|a| a == "--message").unwrap();
    assert_eq!(args[message + 1], "first half");
  }

  #[test]
  fn move_workspace_changes_sends_files_and_hunks_on_the_cli() {
    let args = TreqCommandRequest::MoveWorkspaceChanges {
      repo: "/srv/project".into(),
      workspace: "feat-a".into(),
      destination: "feat-b".into(),
      files: vec!["a.rs".into(), "b.rs".into()],
      hunks: vec![crate::core::workspaces::HunkSpec {
        file_path: "c.rs".into(),
        start_line: 3,
        end_line: 5,
      }],
      commits: vec![],
      idempotency_key: "move-1".into(),
    }
    .cli_args()
    .unwrap();
    let files = args.iter().position(|a| a == "--files").unwrap();
    assert_eq!(args[files + 1], "a.rs,b.rs");
    let path = args.iter().position(|a| a == "--path").unwrap();
    assert_eq!(args[path + 1], "c.rs:3-5");
    assert!(!args.iter().any(|a| a == "--value"));
  }

  #[test]
  fn new_review_reads_are_not_mutations() {
    let reads = [
      TreqCommandRequest::WorkspaceDiff {
        repo: "/r".into(),
        workspace: "1".into(),
      },
      TreqCommandRequest::CommitDiff {
        repo: "/r".into(),
        workspace: None,
        revision: "abc".into(),
      },
      TreqCommandRequest::CommitFileDiff {
        repo: "/r".into(),
        workspace: None,
        revision: "abc".into(),
        path: "a.rs".into(),
      },
      TreqCommandRequest::SearchFiles {
        repo: "/r".into(),
        workspace: None,
        query: "a".into(),
        limit: Some(4),
      },
    ];
    for request in reads {
      assert!(!request.is_mutation(), "{request:?}");
      assert!(!request.requires_idempotency_key(), "{request:?}");
      assert!(TreqCommandRequest::KIND_NAMES.contains(&request.kind_name()));
      request.cli_args().unwrap();
    }
  }

  #[test]
  fn listing_requests_fail_for_a_missing_repository() {
    let missing = "/nonexistent/treq-missing-repo";
    for request in [
      TreqCommandRequest::RepositoryStatus {
        repo: missing.into(),
      },
      TreqCommandRequest::ListWorkspaces {
        repo: missing.into(),
      },
      TreqCommandRequest::ListBranches {
        repo: missing.into(),
      },
    ] {
      let err = execute_local_request(request).unwrap_err();
      assert!(err.starts_with("repository_not_found:"), "{err}");
    }
  }

  // -- Real repository round trips ---------------------------------------------

  mod real_repo {
    use super::super::*;
    use crate::test_repo::TestRepo;

    fn key(tag: &str) -> String {
      format!("{tag}-{}", super::uuid_like())
    }

    fn workspace_path(repo: &TestRepo, id: i64) -> String {
      let ws = crate::local_db::get_workspace_by_id(&repo.repo_path, id)
        .unwrap()
        .unwrap();
      repo.workspace_full_path(&ws)
    }

    fn list_commits(repo: &TestRepo, id: i64) -> serde_json::Value {
      execute_local_request(TreqCommandRequest::ListCommits {
        repo: repo.repo_path.clone(),
        workspace: Some(id.to_string()),
      })
      .unwrap()
    }

    #[test]
    fn create_commit_marker_check_distinguishes_before_and_after_a_real_commit() {
      let repo = TestRepo::new().unwrap();
      let ws = repo.create_workspace_simple("feat-commit").unwrap();
      let path = workspace_path(&repo, ws.id);
      TestRepo::write_workspace_file(&path, "a.txt", "one\n").unwrap();

      let marker: WorkspaceChangeMarker = serde_json::from_value(
        execute_local_request(TreqCommandRequest::WorkspaceChangeMarker {
          repo: repo.repo_path.clone(),
          workspace: Some(ws.id.to_string()),
        })
        .unwrap(),
      )
      .unwrap();
      let base = marker
        .working_copy_change_id
        .expect("marker reports the working-copy change id");

      let request = TreqCommandRequest::CreateCommit {
        repo: repo.repo_path.clone(),
        workspace: Some(ws.id.to_string()),
        message: "wip".into(),
        base_change_id: Some(base),
        idempotency_key: key("commit"),
      };
      let verification = verification_for(&request).unwrap();
      assert_eq!(
        (verification.check)(&list_commits(&repo, ws.id)),
        MutationVerificationOutcome::NotApplied
      );

      execute_local_request(request).unwrap();
      assert_eq!(
        (verification.check)(&list_commits(&repo, ws.id)),
        MutationVerificationOutcome::AlreadyApplied
      );
    }

    #[test]
    fn split_commit_uses_the_message_and_refuses_other_commits() {
      let repo = TestRepo::new().unwrap();
      let ws = repo.create_workspace_simple("feat-split").unwrap();
      let path = workspace_path(&repo, ws.id);
      TestRepo::write_workspace_file(&path, "a.txt", "a\n").unwrap();
      TestRepo::write_workspace_file(&path, "b.txt", "b\n").unwrap();
      // The desktop snapshots the working copy when it lists changes before a
      // split; do the same so the split sees both files.
      crate::jj::jj_get_changed_files(&path).unwrap();

      let refused = execute_local_request(TreqCommandRequest::SplitCommit {
        repo: repo.repo_path.clone(),
        workspace: ws.id.to_string(),
        commit: "zzzzzzzzzzzz".into(),
        files: vec!["a.txt".into()],
        hunks: vec![],
        message: "first".into(),
        idempotency_key: key("split-other"),
      })
      .unwrap_err();
      assert!(refused.starts_with("unsupported:"), "got: {refused}");

      execute_local_request(TreqCommandRequest::SplitCommit {
        repo: repo.repo_path.clone(),
        workspace: ws.id.to_string(),
        commit: "@".into(),
        files: vec!["a.txt".into()],
        hunks: vec![],
        message: "first".into(),
        idempotency_key: key("split"),
      })
      .unwrap();
      let commits = list_commits(&repo, ws.id);
      let described = commits["commits"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["description"].as_str().map(str::trim_end) == Some("first"));
      assert!(described, "split commit must carry the message: {commits}");
    }

    fn patch_file(
      repo: &TestRepo,
      ws: i64,
      path: &str,
      patch: &str,
      tag: &str,
    ) -> Result<serde_json::Value, String> {
      execute_local_request(TreqCommandRequest::PatchFile {
        repo: repo.repo_path.clone(),
        workspace: Some(ws.to_string()),
        path: path.into(),
        patch_base64: patch.into(),
        idempotency_key: key(tag),
      })
    }

    // "--- a/a.txt / +++ b/a.txt / -hi / +patched"
    const PATCH_A_TXT: &str =
      "LS0tIGEvYS50eHQKKysrIGIvYS50eHQKQEAgLTEgKzEgQEAKLWhpCitwYXRjaGVkCg==";

    #[test]
    fn patch_file_applies_the_base64_patch_to_the_path() {
      let repo = TestRepo::new().unwrap();
      let ws = repo.create_workspace_simple("feat-patch").unwrap();
      let path = workspace_path(&repo, ws.id);
      TestRepo::write_workspace_file(&path, "a.txt", "hi\n").unwrap();

      patch_file(&repo, ws.id, "a.txt", PATCH_A_TXT, "patch").unwrap();
      assert_eq!(
        std::fs::read_to_string(Path::new(&path).join("a.txt")).unwrap(),
        "patched\n"
      );
      let changes = crate::core::changes::list_changed_files(&repo.repo_path, Some(ws.id)).unwrap();
      assert!(
        changes.iter().any(|c| c.path == "a.txt"),
        "jj should record the patched file: {changes:?}"
      );
    }

    #[test]
    fn patch_file_creates_a_new_file() {
      let repo = TestRepo::new().unwrap();
      let ws = repo.create_workspace_simple("feat-patch-new").unwrap();
      let path = workspace_path(&repo, ws.id);
      // "--- /dev/null / +++ b/new/file.txt / +created"
      let patch = "LS0tIC9kZXYvbnVsbAorKysgYi9uZXcvZmlsZS50eHQKQEAgLTAsMCArMSBAQAorY3JlYXRlZAo=";
      patch_file(&repo, ws.id, "new/file.txt", patch, "patch-new").unwrap();
      assert_eq!(
        std::fs::read_to_string(Path::new(&path).join("new/file.txt")).unwrap(),
        "created\n"
      );
    }

    #[test]
    fn patch_file_refuses_a_patch_for_another_path() {
      let repo = TestRepo::new().unwrap();
      let ws = repo.create_workspace_simple("feat-patch-other").unwrap();
      let path = workspace_path(&repo, ws.id);
      TestRepo::write_workspace_file(&path, "a.txt", "hi\n").unwrap();
      TestRepo::write_workspace_file(&path, "b.txt", "hi\n").unwrap();

      let err = patch_file(&repo, ws.id, "b.txt", PATCH_A_TXT, "patch-other").unwrap_err();
      assert!(err.starts_with("invalid_arguments:"), "{err}");
      assert_eq!(
        std::fs::read_to_string(Path::new(&path).join("a.txt")).unwrap(),
        "hi\n"
      );
    }

    #[test]
    fn patch_file_refuses_paths_outside_the_workspace() {
      let repo = TestRepo::new().unwrap();
      let ws = repo.create_workspace_simple("feat-patch-escape").unwrap();
      // "--- a/../escape.txt / +++ b/../escape.txt / +x"
      let patch = "LS0tIGEvLi4vZXNjYXBlLnR4dAorKysgYi8uLi9lc2NhcGUudHh0CkBAIC0wLDAgKzEgQEAKK3gK";
      let err = patch_file(&repo, ws.id, "../escape.txt", patch, "patch-escape").unwrap_err();
      assert!(err.starts_with("invalid_arguments:"), "{err}");
      let outside = Path::new(&workspace_path(&repo, ws.id))
        .parent()
        .unwrap()
        .join("escape.txt");
      assert!(!outside.exists());
    }

    #[test]
    fn move_workspace_changes_moves_selected_files() {
      let repo = TestRepo::new().unwrap();
      let source = repo.create_workspace_simple("feat-src").unwrap();
      let destination = repo.create_workspace_simple("feat-dst").unwrap();
      let source_path = workspace_path(&repo, source.id);
      let destination_path = workspace_path(&repo, destination.id);
      TestRepo::write_workspace_file(&source_path, "moved.txt", "moved\n").unwrap();
      TestRepo::write_workspace_file(&source_path, "kept.txt", "kept\n").unwrap();

      let result = execute_local_request(TreqCommandRequest::MoveWorkspaceChanges {
        repo: repo.repo_path,
        workspace: "feat-src".into(),
        destination: "feat-dst".into(),
        files: vec!["moved.txt".into()],
        hunks: vec![],
        commits: vec![],
        idempotency_key: key("move"),
      })
      .unwrap();
      assert_eq!(result["files_moved"], 1, "{result}");
      assert!(Path::new(&destination_path).join("moved.txt").exists());
      assert!(Path::new(&source_path).join("kept.txt").exists());
    }

    #[test]
    fn rebase_workspace_lifts_bridges_onto_the_real_default_branch() {
      let repo = TestRepo::new().unwrap();
      assert_ne!(repo.default_branch(), "main");
      let parent = repo.create_workspace_simple("feat-parent").unwrap();
      // A workspace with no recorded target falls back to the repository
      // default, which is the branch the old hardcoded "main" got wrong.
      rusqlite::Connection::open(crate::local_db::get_local_db_path(&repo.repo_path))
        .unwrap()
        .execute(
          "UPDATE workspaces SET target_branch = NULL WHERE id = ?1",
          [parent.id],
        )
        .unwrap();
      let child = crate::core::workspaces::create_workspace(
        &repo.repo_path,
        "feat-child",
        None,
        None,
        Some("feat-parent"),
        None,
        None,
      )
      .unwrap();
      assert_eq!(child.target_branch.as_deref(), Some("feat-parent"));

      // Moving the parent under its own child lifts the child (the bridge)
      // onto the parent's old target, which here is the repository default.
      execute_local_request(TreqCommandRequest::RebaseWorkspace {
        repo: repo.repo_path.clone(),
        workspace: parent.id.to_string(),
        target_branch: "feat-child".into(),
        idempotency_key: key("rebase"),
      })
      .unwrap();
      let lifted = crate::local_db::get_workspace_by_id(&repo.repo_path, child.id)
        .unwrap()
        .unwrap();
      assert_eq!(lifted.target_branch.as_deref(), Some(repo.default_branch()));
    }
  }
}
