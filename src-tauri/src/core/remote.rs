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
  AbandonCommit {
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
  GitBookmarkTrack {
    repo: String,
    bookmark: String,
    remote_name: String,
  },
  GitPush {
    repo: String,
    workspace: Option<String>,
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
  /// Starts (or reuses, if already running) a persistent tmux/screen-backed
  /// PTY session. Does not itself attach — the client attaches over a
  /// separate SSH PTY channel using `PtyAttachCommand`'s returned command
  /// line, matching how `core::remote_pty::build_launch_command` is used.
  PtyStart {
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
    repo: String,
    workspace: Option<String>,
  },
  /// Stops (kills) a persistent PTY session. Idempotent.
  PtyStop {
    repo: String,
    workspace: String,
    label: String,
  },
  /// Returns the literal command line an SSH PTY channel should exec to
  /// attach (creating first if necessary) to a persistent session.
  PtyAttachCommand {
    repo: String,
    workspace: String,
    label: String,
    remote_dir: String,
    launch: crate::core::remote_pty::PtyLaunchSpec,
    cols: u16,
    rows: u16,
  },
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
      | Self::InspectWorkspace { .. }
      | Self::ListChanges { .. }
      | Self::DiffFile { .. }
      | Self::ReadFile { .. }
      | Self::ListCommits { .. }
      | Self::ListConflicts { .. }
      | Self::WorkspaceDiff { .. }
      | Self::CommitDiff { .. }
      | Self::CommitFileDiff { .. }
      | Self::SearchFiles { .. }
      | Self::ProbeRepo { .. }
      | Self::AgentStatus { .. }
      | Self::AgentLogs { .. }
      | Self::PtyList { .. }
      | Self::PtyAttachCommand { .. }
      | Self::WorkspaceChangeMarker { .. }
      | Self::MachineUsage { .. } => false,
      // -- Mutations -----------------------------------------------------------
      Self::CloneRepo { .. }
      | Self::InitRepo { .. }
      | Self::CreateWorkspace { .. }
      | Self::RenameWorkspace { .. }
      | Self::UpdateWorkspace { .. }
      | Self::DeleteWorkspace { .. }
      | Self::MoveWorkspaceChanges { .. }
      | Self::RebaseWorkspace { .. }
      | Self::RestoreFile { .. }
      | Self::PatchFile { .. }
      | Self::CreateCommit { .. }
      | Self::DescribeCommit { .. }
      | Self::SplitCommit { .. }
      | Self::MoveCommit { .. }
      | Self::AbandonCommit { .. }
      | Self::ResolveConflict { .. }
      | Self::GitFetch { .. }
      | Self::GitBookmarkTrack { .. }
      | Self::GitPush { .. }
      | Self::AgentStart { .. }
      | Self::AgentInput { .. }
      | Self::AgentStop { .. }
      | Self::PtyStart { .. }
      | Self::PtyStop { .. } => true,
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
      | Self::CreateWorkspace { .. }
      | Self::RenameWorkspace { .. }
      | Self::MoveWorkspaceChanges { .. }
      | Self::RebaseWorkspace { .. }
      | Self::PatchFile { .. }
      | Self::CreateCommit { .. }
      | Self::SplitCommit { .. }
      | Self::MoveCommit { .. }
      | Self::AbandonCommit { .. }
      | Self::ResolveConflict { .. }
      | Self::GitPush { .. }
      | Self::AgentStart { .. }
      | Self::AgentInput { .. }
      | Self::PtyStart { .. } => true,
      Self::InspectRepository { .. }
      | Self::RepositoryStatus { .. }
      | Self::ListBranches { .. }
      | Self::ListWorkspaces { .. }
      | Self::InspectWorkspace { .. }
      | Self::ListChanges { .. }
      | Self::DiffFile { .. }
      | Self::ReadFile { .. }
      | Self::ListCommits { .. }
      | Self::ListConflicts { .. }
      | Self::WorkspaceDiff { .. }
      | Self::CommitDiff { .. }
      | Self::CommitFileDiff { .. }
      | Self::SearchFiles { .. }
      | Self::WorkspaceChangeMarker { .. }
      | Self::MachineUsage { .. }
      | Self::ProbeRepo { .. }
      | Self::UpdateWorkspace { .. }
      | Self::DeleteWorkspace { .. }
      | Self::RestoreFile { .. }
      | Self::DescribeCommit { .. }
      | Self::GitFetch { .. }
      | Self::GitBookmarkTrack { .. }
      | Self::AgentStatus { .. }
      | Self::AgentStop { .. }
      | Self::AgentLogs { .. }
      | Self::PtyList { .. }
      | Self::PtyStop { .. }
      | Self::PtyAttachCommand { .. } => false,
    }
  }

  pub fn kind_name(&self) -> &'static str {
    match self {
      Self::InspectRepository { .. } => "InspectRepository",
      Self::RepositoryStatus { .. } => "RepositoryStatus",
      Self::ListBranches { .. } => "ListBranches",
      Self::ListWorkspaces { .. } => "ListWorkspaces",
      Self::InspectWorkspace { .. } => "InspectWorkspace",
      Self::ListChanges { .. } => "ListChanges",
      Self::DiffFile { .. } => "DiffFile",
      Self::ReadFile { .. } => "ReadFile",
      Self::ListCommits { .. } => "ListCommits",
      Self::ListConflicts { .. } => "ListConflicts",
      Self::WorkspaceDiff { .. } => "WorkspaceDiff",
      Self::CommitDiff { .. } => "CommitDiff",
      Self::CommitFileDiff { .. } => "CommitFileDiff",
      Self::SearchFiles { .. } => "SearchFiles",
      Self::WorkspaceChangeMarker { .. } => "WorkspaceChangeMarker",
      Self::MachineUsage { .. } => "MachineUsage",
      Self::ProbeRepo { .. } => "ProbeRepo",
      Self::CloneRepo { .. } => "CloneRepo",
      Self::InitRepo { .. } => "InitRepo",
      Self::CreateWorkspace { .. } => "CreateWorkspace",
      Self::RenameWorkspace { .. } => "RenameWorkspace",
      Self::UpdateWorkspace { .. } => "UpdateWorkspace",
      Self::DeleteWorkspace { .. } => "DeleteWorkspace",
      Self::MoveWorkspaceChanges { .. } => "MoveWorkspaceChanges",
      Self::RebaseWorkspace { .. } => "RebaseWorkspace",
      Self::RestoreFile { .. } => "RestoreFile",
      Self::PatchFile { .. } => "PatchFile",
      Self::CreateCommit { .. } => "CreateCommit",
      Self::DescribeCommit { .. } => "DescribeCommit",
      Self::SplitCommit { .. } => "SplitCommit",
      Self::MoveCommit { .. } => "MoveCommit",
      Self::AbandonCommit { .. } => "AbandonCommit",
      Self::ResolveConflict { .. } => "ResolveConflict",
      Self::GitFetch { .. } => "GitFetch",
      Self::GitBookmarkTrack { .. } => "GitBookmarkTrack",
      Self::GitPush { .. } => "GitPush",
      Self::AgentStart { .. } => "AgentStart",
      Self::AgentInput { .. } => "AgentInput",
      Self::AgentStatus { .. } => "AgentStatus",
      Self::AgentStop { .. } => "AgentStop",
      Self::AgentLogs { .. } => "AgentLogs",
      Self::PtyStart { .. } => "PtyStart",
      Self::PtyList { .. } => "PtyList",
      Self::PtyStop { .. } => "PtyStop",
      Self::PtyAttachCommand { .. } => "PtyAttachCommand",
    }
  }

  /// Stable list of every typed command kind. TypeScript
  /// `TREQ_COMMAND_KINDS` must match this exactly.
  pub const KIND_NAMES: &'static [&'static str] = &[
    "InspectRepository",
    "RepositoryStatus",
    "ListBranches",
    "ListWorkspaces",
    "InspectWorkspace",
    "ListChanges",
    "DiffFile",
    "ReadFile",
    "ListCommits",
    "ListConflicts",
    "WorkspaceChangeMarker",
    "MachineUsage",
    "ProbeRepo",
    "CloneRepo",
    "InitRepo",
    "CreateWorkspace",
    "RenameWorkspace",
    "UpdateWorkspace",
    "DeleteWorkspace",
    "MoveWorkspaceChanges",
    "RebaseWorkspace",
    "RestoreFile",
    "PatchFile",
    "CreateCommit",
    "DescribeCommit",
    "SplitCommit",
    "MoveCommit",
    "AbandonCommit",
    "ResolveConflict",
    "GitFetch",
    "GitBookmarkTrack",
    "GitPush",
    "AgentStart",
    "AgentInput",
    "AgentStatus",
    "AgentStop",
    "AgentLogs",
    "PtyStart",
    "PtyList",
    "PtyStop",
    "PtyAttachCommand",
    "WorkspaceDiff",
    "CommitDiff",
    "CommitFileDiff",
    "SearchFiles",
  ];
}

/// The result of checking observable VM state against a mutation's expected
/// effect after a network failure interrupted the original attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationVerificationOutcome {
  /// State shows the mutation already applied: the client treats it as
  /// complete and does not resend.
  AlreadyApplied,
  /// State shows the mutation did not apply: the client may retry, using the
  /// same idempotency key.
  NotApplied,
  /// State is ambiguous: the client surfaces this to the user rather than
  /// guessing.
  Ambiguous,
}

/// A verification recipe for one in-flight mutation: a typed read command
/// safe to retry, plus a function that inspects its JSON result and decides
/// whether the mutation already landed. Built once per mutation attempt (it
/// closes over the mutation's own arguments, e.g. the branch name a
/// `CreateWorkspace` call was trying to create) so the check can compare
/// "what we asked for" against "what state now shows".
pub struct MutationVerification {
  pub read_request: TreqCommandRequest,
  pub check: Box<dyn Fn(&serde_json::Value) -> MutationVerificationOutcome + Send + Sync>,
}

fn json_str<'a>(value: &'a serde_json::Value, field: &str) -> Option<&'a str> {
  value.get(field).and_then(|v| v.as_str())
}

/// Workspace ids are numbers in `ListWorkspaces` output but strings in typed
/// requests; compare them as text.
fn json_id_matches(item: &serde_json::Value, id: &str) -> bool {
  match item.get("id") {
    Some(serde_json::Value::String(value)) => value == id,
    Some(serde_json::Value::Number(value)) => value.to_string() == id,
    _ => false,
  }
}

/// Builds the verification recipe for `request`, when one exists. Every
/// mutation under "Remote mutation coverage" that this module currently
/// implements has one; a mutation with no meaningful state to observe (e.g.
/// `RestoreFile`, whose "effect" is the absence of a diff a read command
/// would need workspace-specific context to reconstruct reliably) returns
/// `None`, and the caller treats that as always-ambiguous rather than
/// guessing a false-positive check.
pub fn verification_for(request: &TreqCommandRequest) -> Option<MutationVerification> {
  match request {
    TreqCommandRequest::CreateWorkspace {
      repo, branch_name, ..
    } => {
      let branch_name = branch_name.clone();
      Some(MutationVerification {
        read_request: TreqCommandRequest::ListWorkspaces { repo: repo.clone() },
        check: Box::new(move |value| {
          let Some(items) = value.as_array() else {
            return MutationVerificationOutcome::Ambiguous;
          };
          let found = items
            .iter()
            .any(|item| json_str(item, "branch_name") == Some(branch_name.as_str()));
          if found {
            MutationVerificationOutcome::AlreadyApplied
          } else {
            MutationVerificationOutcome::NotApplied
          }
        }),
      })
    }
    TreqCommandRequest::DeleteWorkspace { repo, workspace } => {
      let workspace = workspace.clone();
      Some(MutationVerification {
        read_request: TreqCommandRequest::ListWorkspaces { repo: repo.clone() },
        check: Box::new(move |value| {
          let Some(items) = value.as_array() else {
            return MutationVerificationOutcome::Ambiguous;
          };
          let still_present = items
            .iter()
            .any(|item| json_id_matches(item, workspace.as_str()));
          if still_present {
            MutationVerificationOutcome::NotApplied
          } else {
            MutationVerificationOutcome::AlreadyApplied
          }
        }),
      })
    }
    TreqCommandRequest::RenameWorkspace {
      repo,
      workspace,
      new_name,
      ..
    } => {
      let workspace = workspace.clone();
      let new_name = new_name.clone();
      Some(MutationVerification {
        read_request: TreqCommandRequest::InspectWorkspace {
          repo: repo.clone(),
          workspace,
        },
        // Renaming changes the workspace's branch; `InspectWorkspace`
        // returns it under `current`.
        check: Box::new(move |value| {
          match value
            .get("current")
            .and_then(|c| json_str(c, "branch_name"))
          {
            Some(current) if current == new_name => MutationVerificationOutcome::AlreadyApplied,
            Some(_) => MutationVerificationOutcome::NotApplied,
            None => MutationVerificationOutcome::Ambiguous,
          }
        }),
      })
    }
    // Matching on the message alone is not enough: repeated messages such as
    // "wip" would make an older commit look like this one. Without the
    // pre-commit working-copy change id there is no precise check, so the
    // caller reports ambiguity rather than guessing.
    TreqCommandRequest::CreateCommit {
      repo,
      workspace,
      message,
      base_change_id: Some(base_change_id),
      ..
    } => {
      let message = message.clone();
      let base_change_id = base_change_id.clone();
      Some(MutationVerification {
        read_request: TreqCommandRequest::ListCommits {
          repo: repo.clone(),
          workspace: workspace.clone(),
        },
        check: Box::new(move |value| check_create_commit(value, &base_change_id, &message)),
      })
    }
    TreqCommandRequest::AbandonCommit {
      repo,
      workspace,
      commit,
      ..
    } => {
      let commit = commit.clone();
      Some(MutationVerification {
        read_request: TreqCommandRequest::ListCommits {
          repo: repo.clone(),
          workspace: Some(workspace.clone()),
        },
        check: Box::new(move |value| {
          let Some(items) = value.get("commits").and_then(|v| v.as_array()) else {
            return MutationVerificationOutcome::Ambiguous;
          };
          let still_present = items.iter().any(|item| {
            json_str(item, "commit_id") == Some(commit.as_str())
              || json_str(item, "change_id") == Some(commit.as_str())
          });
          if still_present {
            MutationVerificationOutcome::NotApplied
          } else {
            MutationVerificationOutcome::AlreadyApplied
          }
        }),
      })
    }
    TreqCommandRequest::ResolveConflict { repo, revision, .. } => {
      let revision = revision.clone();
      Some(MutationVerification {
        read_request: TreqCommandRequest::ListCommits {
          repo: repo.clone(),
          workspace: None,
        },
        check: Box::new(move |value| check_resolve_conflict(value, &revision)),
      })
    }
    TreqCommandRequest::GitBookmarkTrack { repo, bookmark, .. } => {
      let bookmark = bookmark.clone();
      Some(MutationVerification {
        read_request: TreqCommandRequest::ListBranches { repo: repo.clone() },
        check: Box::new(move |value| {
          let Some(items) = value.as_array() else {
            return MutationVerificationOutcome::Ambiguous;
          };
          let tracked = items.iter().any(|item| {
            json_str(item, "name") == Some(bookmark.as_str())
              && item.get("tracked").and_then(|v| v.as_bool()) == Some(true)
          });
          if tracked {
            MutationVerificationOutcome::AlreadyApplied
          } else {
            MutationVerificationOutcome::NotApplied
          }
        }),
      })
    }
    TreqCommandRequest::AgentStart {
      repo, workspace, ..
    } => Some(MutationVerification {
      read_request: TreqCommandRequest::AgentStatus {
        repo: repo.clone(),
        workspace: workspace.clone(),
      },
      check: Box::new(
        move |value| match value.get("running").and_then(|v| v.as_bool()) {
          Some(true) => MutationVerificationOutcome::AlreadyApplied,
          Some(false) => MutationVerificationOutcome::NotApplied,
          None => MutationVerificationOutcome::Ambiguous,
        },
      ),
    }),
    TreqCommandRequest::AgentStop {
      repo, workspace, ..
    } => Some(MutationVerification {
      read_request: TreqCommandRequest::AgentStatus {
        repo: repo.clone(),
        workspace: workspace.clone(),
      },
      check: Box::new(
        move |value| match value.get("running").and_then(|v| v.as_bool()) {
          Some(false) => MutationVerificationOutcome::AlreadyApplied,
          Some(true) => MutationVerificationOutcome::NotApplied,
          None => MutationVerificationOutcome::Ambiguous,
        },
      ),
    }),
    // No reliable, cheap state check exists yet for these mutations over the
    // typed read surface (e.g. a file restore's "effect" is the absence of a
    // diff, which needs the same file/workspace context the mutation itself
    // used and is easy to get subtly wrong). Rather than guess and risk a
    // false "already applied"/"not applied" call, these fall back to
    // `MutationVerificationOutcome::Ambiguous` in `retry_after_reconnect`.
    _ => None,
  }
}

/// Decides whether a `CreateCommit` landed by finding the working-copy change
/// that was current before the commit ran. A commit keeps that change id,
/// sets its description, and moves the working copy to a new child, so:
///
/// - the change is still the working copy: the commit did not run;
/// - the change is no longer the working copy and carries the message: it ran;
/// - anything else (change missing, different description): ambiguous.
fn check_create_commit(
  value: &serde_json::Value,
  base_change_id: &str,
  message: &str,
) -> MutationVerificationOutcome {
  let Some(items) = value.get("commits").and_then(|v| v.as_array()) else {
    return MutationVerificationOutcome::Ambiguous;
  };
  let tentative = value
    .get("tentative_working_copy")
    .and_then(|v| v.get("commit"));
  let base = items.iter().chain(tentative).find(|item| {
    json_str(item, "change_id").is_some_and(|id| change_ids_match(id, base_change_id))
  });
  let Some(base) = base else {
    return MutationVerificationOutcome::Ambiguous;
  };
  let is_working_copy = base
    .get("is_working_copy")
    .and_then(|v| v.as_bool())
    .unwrap_or(false);
  if is_working_copy {
    return MutationVerificationOutcome::NotApplied;
  }
  let description = json_str(base, "description").unwrap_or("");
  if description.trim_end() == message.trim_end() {
    MutationVerificationOutcome::AlreadyApplied
  } else {
    MutationVerificationOutcome::Ambiguous
  }
}

/// Change ids may be shown at different prefix lengths; treat a shorter id as
/// matching when it is a prefix of the longer one.
/// Decides whether a `ResolveConflict` landed by reading the revision's
/// `has_conflicts` flag from the home-repo log. `ListConflicts` cannot answer
/// this: it lists file paths, not change ids. A revision the log does not
/// show (or a non-change-id like `@`) stays ambiguous.
fn check_resolve_conflict(
  value: &serde_json::Value,
  revision: &str,
) -> MutationVerificationOutcome {
  let Some(items) = value.get("commits").and_then(|v| v.as_array()) else {
    return MutationVerificationOutcome::Ambiguous;
  };
  let tentative = value
    .get("tentative_working_copy")
    .and_then(|v| v.get("commit"));
  let Some(commit) = items
    .iter()
    .chain(tentative)
    .find(|item| json_str(item, "change_id").is_some_and(|id| change_ids_match(id, revision)))
  else {
    return MutationVerificationOutcome::Ambiguous;
  };
  match commit.get("has_conflicts").and_then(|v| v.as_bool()) {
    Some(false) => MutationVerificationOutcome::AlreadyApplied,
    Some(true) => MutationVerificationOutcome::NotApplied,
    None => MutationVerificationOutcome::Ambiguous,
  }
}

fn change_ids_match(a: &str, b: &str) -> bool {
  if a.is_empty() || b.is_empty() {
    return false;
  }
  a.starts_with(b) || b.starts_with(a)
}

/// Outcome of running a mutation with verify-before-retry semantics after a
/// network failure. Carries enough for a Phase 6 UI to render each case from
/// the PRD: treat-as-complete, retry-with-idempotency-key, or
/// surface-ambiguity-to-user.
#[derive(Debug, Clone, PartialEq)]
pub enum MutationRetryOutcome<T> {
  /// The mutation exec channel completed normally (no network failure, or a
  /// retry that itself completed). Carries the fresh response.
  Applied(T),
  /// A network failure interrupted the exec channel, but the post-reconnect
  /// verification read showed the mutation's effect was already observable
  /// in VM state. The client does not resend.
  AlreadyApplied,
  /// A network failure interrupted the exec channel, and the post-reconnect
  /// verification read could not determine whether the mutation applied
  /// (the read itself failed, returned an unexpected shape, or this
  /// mutation has no verification recipe). The client must surface this to
  /// the user rather than guess.
  Ambiguous { reason: String },
}

/// Reported once per verify-before-retry decision so a caller can feed it
/// into telemetry (PRD "Client and transport telemetry": "post-reconnect
/// state verifications before a mutation retry, and their outcome").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostReconnectVerification {
  AlreadyApplied,
  Retried,
  Ambiguous,
}

/// Runs `request` over `pool`/`endpoint` with the PRD's verify-before-retry
/// behavior: a network failure while the mutation was in flight (as opposed
/// to a structured error the CLI itself returned) never triggers a blind
/// resend. Instead, once the connection is usable again, an appropriate
/// typed read command checks whether the mutation's effect is already
/// observable, and only a "not applied" verdict retries — with the same
/// idempotency key the original request carried, so a command that actually
/// landed once on the VM is not reapplied by a late retry racing the
/// original.
///
/// `on_verification` is called at most once, exactly when a network failure
/// forced a verification read, with the decision that resulted — this is the
/// hook a caller uses to emit the PRD's "post-reconnect state verifications
/// ... and their outcome" telemetry.
pub async fn retry_after_reconnect<T, F>(
  pool: &crate::core::remote_ssh_transport::SshConnectionPool,
  endpoint: &crate::core::remote_control_plane::SshEndpoint,
  request: TreqCommandRequest,
  limits: crate::core::remote_ssh_transport::ExecLimits,
  cancellation: &crate::core::remote_ssh_transport::CancellationToken,
  mut on_verification: F,
) -> Result<MutationRetryOutcome<T>, RemoteCommandError>
where
  T: DeserializeOwned,
  F: FnMut(PostReconnectVerification),
{
  debug_assert!(
    request.is_mutation(),
    "retry_after_reconnect is only meaningful for mutations; reads are always safe to retry directly"
  );

  // Record the pre-mutation marker the verification check needs before the
  // mutation can change it. A failed read leaves it unset, which only makes a
  // later verification report ambiguity; it never blocks the mutation.
  let request = match baseline_request(&request) {
    Some(read) => {
      match execute_remote_command::<WorkspaceChangeMarker>(
        pool,
        endpoint,
        read,
        limits,
        cancellation,
      )
      .await
      {
        Ok(marker) => with_verification_baseline(request, marker),
        Err(_) => request,
      }
    }
    None => request,
  };

  match execute_remote_command::<T>(
    pool,
    endpoint,
    clone_request(&request),
    limits,
    cancellation,
  )
  .await
  {
    Ok(value) => Ok(MutationRetryOutcome::Applied(value)),
    // The CLI reached the VM and returned a structured application-level
    // error (e.g. `workspace_not_found`). That is not the "did it land?"
    // uncertainty this function exists for — the CLI told us plainly it did
    // not run — so it is surfaced as-is rather than triggering verification.
    Err(error @ RemoteCommandError::Command { .. }) => Err(error),
    Err(error @ RemoteCommandError::InvalidJson(_)) => Err(error),
    // The command was refused locally before ever reaching the network
    // (PRD "Hard cutoff on revocation or expiry"): there is no "did it
    // land?" uncertainty to verify, since it certainly did not run.
    Err(error @ RemoteCommandError::CredentialCutOff { .. }) => Err(error),
    // A transport-layer failure: the client cannot tell whether the command
    // reached the VM, ran, or completed. Verify observable state before
    // deciding anything.
    Err(RemoteCommandError::Transport(transport_message)) => {
      let Some(verification) = verification_for(&request) else {
        on_verification(PostReconnectVerification::Ambiguous);
        return Ok(MutationRetryOutcome::Ambiguous {
          reason: format!(
            "network failure during mutation ({transport_message}), and no state check is available to confirm whether it applied"
          ),
        });
      };

      let read_result = execute_remote_command::<serde_json::Value>(
        pool,
        endpoint,
        verification.read_request,
        limits,
        cancellation,
      )
      .await;

      let observed = match read_result {
        Ok(value) => (verification.check)(&value),
        // The verification read itself could not be completed (e.g. still
        // reconnecting) — this is exactly the "state is ambiguous" case, not
        // a reason to guess.
        Err(_) => MutationVerificationOutcome::Ambiguous,
      };

      match observed {
        MutationVerificationOutcome::AlreadyApplied => {
          on_verification(PostReconnectVerification::AlreadyApplied);
          Ok(MutationRetryOutcome::AlreadyApplied)
        }
        MutationVerificationOutcome::NotApplied => {
          on_verification(PostReconnectVerification::Retried);
          // Retried with the exact same request, including its idempotency
          // key field (if any) — so a command that actually did land exactly
          // once on the VM side is not reapplied a second time by this retry
          // racing the original.
          let retried =
            execute_remote_command::<T>(pool, endpoint, request, limits, cancellation).await?;
          Ok(MutationRetryOutcome::Applied(retried))
        }
        MutationVerificationOutcome::Ambiguous => {
          on_verification(PostReconnectVerification::Ambiguous);
          Ok(MutationRetryOutcome::Ambiguous {
            reason: format!(
              "network failure during mutation ({transport_message}), and post-reconnect state was ambiguous"
            ),
          })
        }
      }
    }
  }
}

/// The read that captures a mutation's pre-mutation marker, when its
/// verification needs one and the caller did not already supply it.
pub fn baseline_request(request: &TreqCommandRequest) -> Option<TreqCommandRequest> {
  match request {
    TreqCommandRequest::CreateCommit {
      repo,
      workspace,
      base_change_id: None,
      ..
    } => Some(TreqCommandRequest::WorkspaceChangeMarker {
      repo: repo.clone(),
      workspace: workspace.clone(),
    }),
    _ => None,
  }
}

/// Copies the marker read by [`baseline_request`] into the request so it
/// travels with the mutation, into the idempotency record, and into any
/// stale-claim recovery on the VM.
pub fn with_verification_baseline(
  request: TreqCommandRequest,
  marker: WorkspaceChangeMarker,
) -> TreqCommandRequest {
  match request {
    TreqCommandRequest::CreateCommit {
      repo,
      workspace,
      message,
      base_change_id: None,
      idempotency_key,
    } => TreqCommandRequest::CreateCommit {
      repo,
      workspace,
      message,
      base_change_id: marker.working_copy_change_id,
      idempotency_key,
    },
    other => other,
  }
}

/// `TreqCommandRequest` does not derive `Clone` reuse would require adding it
/// repo-wide; this narrow helper only needs to survive a single retry path,
/// so it round-trips through JSON rather than widening the enum's derive
/// list for one caller.
fn clone_request(request: &TreqCommandRequest) -> TreqCommandRequest {
  serde_json::from_value(serde_json::to_value(request).expect("TreqCommandRequest serializes"))
    .expect("TreqCommandRequest round-trips through its own JSON shape")
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

fn pty_launch_payload(
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
      Self::InspectWorkspace { repo, workspace } => {
        fields.workspace = Some(workspace);
        ("workspace", "inspect", repo)
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
      Self::DiffFile {
        repo,
        workspace,
        path,
      } => {
        fields.workspace = workspace.as_deref();
        fields.path = Some(path);
        ("changes", "diff", repo)
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
        repo,
        workspace,
        label,
        remote_dir,
        launch,
        cols,
        rows,
        idempotency_key,
      } => {
        fields.workspace = Some(workspace);
        fields.target = Some(label);
        fields.value = Some(pty_launch_payload(remote_dir, launch, *cols, *rows));
        fields.idempotency_key = Some(idempotency_key);
        ("pty-remote", "start", repo)
      }
      Self::PtyList { repo, workspace } => {
        fields.workspace = workspace.as_deref();
        ("pty-remote", "list", repo)
      }
      Self::PtyStop {
        repo,
        workspace,
        label,
      } => {
        fields.workspace = Some(workspace);
        fields.target = Some(label);
        ("pty-remote", "stop", repo)
      }
      Self::PtyAttachCommand {
        repo,
        workspace,
        label,
        remote_dir,
        launch,
        cols,
        rows,
      } => {
        fields.workspace = Some(workspace);
        fields.target = Some(label);
        fields.value = Some(pty_launch_payload(remote_dir, launch, *cols, *rows));
        ("pty-remote", "attach-command", repo)
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
  SshConnectionFailed(String),
  CommandFailed { code: Option<i32>, message: String },
  InvalidJson(String),
}

pub trait TreqCommandTransport {
  fn execute<T: DeserializeOwned>(&self, request: TreqCommandRequest) -> Result<T, TransportError>;
}

/// Serializes the same core DTOs the native SSH exec transport
/// (`crate::core::remote_ssh_transport`) does, which keeps local and remote
/// callers on one response contract.
pub struct LocalTransport;

impl TreqCommandTransport for LocalTransport {
  fn execute<T: DeserializeOwned>(&self, request: TreqCommandRequest) -> Result<T, TransportError> {
    let value = execute_local_request(request).map_err_command()?;
    serde_json::from_value(value).map_err(|error| TransportError::InvalidJson(error.to_string()))
  }
}

// ---------------------------------------------------------------------------
// Idempotency
// ---------------------------------------------------------------------------
//
// Mutations that could duplicate work on retry (create, movement, git push,
// agent start) accept an idempotency key. A repeated call with the same key
// replays the recorded response instead of re-running the mutation.
//
// Persisted durably in `core::idempotency_store::IdempotencyStore`, keyed by
// the same repo-local `.treq/local.db` convention `local_db.rs` uses — not
// an in-process cache — because the remote VM starts a fresh `treq` CLI
// process per SSH exec channel, and a process-local cache cannot deduplicate
// a retry, or a concurrent invocation, that lands in a different process.
//
// Naturally idempotent operations (describe/update, which simply overwrite;
// fetch/inspect/list, which are read-only) do not accept a key at all —
// adding dedupe machinery for those would be needless per the PRD's own
// carve-out.
use crate::core::idempotency_store::{
  fingerprint, redact_for_storage, IdempotencyStore, RecoveryDecision,
};

/// Runs `run` unless `key` was already used for `operation` on an equivalent
/// `request`, in which case the previously recorded (or verification-derived)
/// response is replayed and `run` is never invoked again. Durable across CLI
/// process boundaries: `repo` selects the same `.treq/local.db` any other
/// invocation against that repository would open.
///
/// Reusing `key` for a different operation, or the same operation with a
/// different request body, returns a structured `idempotency_conflict`
/// error. A pending claim left behind by a process that died mid-mutation
/// (lost SSH connection, crashed CLI) is resolved by re-running the
/// mutation's existing observable-state verification recipe
/// ([`verification_for`]) once it goes stale, exactly as a post-reconnect
/// retry would.
fn with_idempotency_key<F>(
  repo: &str,
  operation: &str,
  key: Option<&str>,
  request: &TreqCommandRequest,
  run: F,
) -> Result<serde_json::Value, String>
where
  F: FnOnce() -> Result<serde_json::Value, String>,
{
  let Some(key) = key.filter(|k| !k.trim().is_empty()) else {
    return run();
  };

  let request_json = serde_json::to_value(request).map_err(|e| e.to_string())?;
  let request_fingerprint = fingerprint(&request_json);
  let store = IdempotencyStore::open(repo)?;

  let claim = store.claim_or_replay(
    operation,
    key,
    &request_fingerprint,
    crate::core::idempotency_store::DEFAULT_STALE_AFTER,
    || recover_stale_claim(request),
  )?;
  if let Some(replay) = claim {
    return Ok(replay);
  }

  let outcome = match run() {
    Ok(result) => {
      store.complete(operation, key, &redact_for_storage(&result))?;
      Ok(result)
    }
    Err(error) => {
      // A structured failure, not a "did it land?" ambiguity: free the key
      // for reuse rather than leaving a pending claim behind forever.
      store.abandon(operation, key)?;
      Err(error)
    }
  };
  // Best-effort, bounded retention: opportunistically sweep old completed
  // records on the same connection rather than running a separate cleanup
  // process. A failure here must never mask the mutation's own outcome.
  let _ = store.cleanup(crate::core::idempotency_store::DEFAULT_RETENTION);
  outcome
}

/// Resolves a stale pending claim by running the mutation's existing
/// observable-state verification recipe locally (the same recipe
/// [`verification_for`] builds for the async post-reconnect retry path,
/// applied here via a synchronous local dispatch instead of an SSH read).
fn recover_stale_claim(request: &TreqCommandRequest) -> Result<RecoveryDecision, String> {
  let Some(verification) = verification_for(request) else {
    return Ok(RecoveryDecision::Ambiguous);
  };
  let observed = match execute_local_request(verification.read_request) {
    Ok(value) => value,
    Err(_) => return Ok(RecoveryDecision::Ambiguous),
  };
  match (verification.check)(&observed) {
    MutationVerificationOutcome::AlreadyApplied => {
      Ok(RecoveryDecision::AlreadyApplied { observed })
    }
    MutationVerificationOutcome::NotApplied => Ok(RecoveryDecision::VerifiedNotApplied),
    MutationVerificationOutcome::Ambiguous => Ok(RecoveryDecision::Ambiguous),
  }
}

fn resolve_workspace_path(repo: &str, id: Option<i64>) -> Result<String, String> {
  match id {
    None => Ok(repo.to_string()),
    Some(id) => {
      let workspace = crate::local_db::get_workspace_by_id(repo, id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("workspace_not_found: Workspace {id} was not found"))?;
      Ok(
        Path::new(repo)
          .join(".treq")
          .join("workspaces")
          .join(workspace.workspace_path)
          .to_string_lossy()
          .into_owned(),
      )
    }
  }
}

fn workspace_id(value: Option<&String>) -> Result<Option<i64>, String> {
  value
    .map(|value| {
      value
        .parse::<i64>()
        .ok()
        .filter(|id| *id > 0)
        .ok_or_else(|| "invalid_arguments: workspace must be a positive numeric id".to_string())
    })
    .transpose()
}

/// Dispatches typed CLI requests to the same core functions used by Tauri.
pub fn execute_local_request(request: TreqCommandRequest) -> Result<serde_json::Value, String> {
  request.cli_args()?;
  fn json<T: Serialize>(result: Result<T, String>) -> Result<serde_json::Value, String> {
    serde_json::to_value(result?).map_err(|error| error.to_string())
  }
  // Snapshotted before the match consumes `request` by value: mutation arms
  // need the whole original request both for the idempotency store's
  // fingerprint and, on a stale claim, for `verification_for`.
  let request_snapshot = if request.is_mutation() {
    Some(clone_request(&request))
  } else {
    None
  };
  match request {
    TreqCommandRequest::InspectRepository { repo } => json(inspect_repository_path(&repo)),
    // These return defaults for a non-repository path, so fail first instead.
    TreqCommandRequest::RepositoryStatus { repo } => {
      require_existing_repo(&repo)?;
      json(crate::core::workspaces::workspace_status(&repo, None))
    }
    TreqCommandRequest::ListBranches { repo } => {
      require_existing_repo(&repo)?;
      json(crate::core::repo::list_repo_branches(&repo))
    }
    TreqCommandRequest::ListWorkspaces { repo } => {
      require_existing_repo(&repo)?;
      json(crate::core::workspaces::list_workspaces(&repo))
    }
    TreqCommandRequest::MachineUsage { root } => json(machine_usage(&root)),
    TreqCommandRequest::InspectWorkspace { repo, workspace } => json(
      crate::core::workspaces::workspace_status(&repo, workspace_id(Some(&workspace))?),
    ),
    TreqCommandRequest::ListChanges { repo, workspace } => json(
      crate::core::changes::list_changed_files(&repo, workspace_id(workspace.as_ref())?),
    ),
    TreqCommandRequest::DiffFile {
      repo,
      workspace,
      path,
    } => json(crate::core::changes::list_file_hunks(
      &repo,
      workspace_id(workspace.as_ref())?,
      &path,
      "git",
    )),
    TreqCommandRequest::ReadFile {
      repo,
      workspace,
      path,
      revision,
      start_line,
      end_line,
    } => {
      let start_line = start_line.unwrap_or(1);
      json(crate::core::changes::get_file_lines(
        &repo,
        workspace_id(workspace.as_ref())?,
        &path,
        revision == FileRevision::Parent,
        start_line,
        end_line.unwrap_or(start_line.saturating_add(299)),
      ))
    }
    TreqCommandRequest::ListCommits { repo, workspace } => {
      json(crate::core::commits::list_commits(
        &repo,
        workspace_id(workspace.as_ref())?,
        false,
        None,
        None,
      ))
    }
    TreqCommandRequest::ListConflicts { repo, workspace } => {
      let id = workspace_id(workspace.as_ref())?;
      let workspace_path = resolve_workspace_path(&repo, id)?;
      let files = crate::jj::get_conflicted_files(&workspace_path, None)
        .map_err(|error| format!("jj_command_failed: {error}"))?;
      serde_json::to_value(files).map_err(|error| error.to_string())
    }
    TreqCommandRequest::WorkspaceChangeMarker { repo, workspace } => {
      let id = workspace_id(workspace.as_ref())?;
      let workspace_path = resolve_workspace_path(&repo, id)?;
      json(Ok::<_, String>(WorkspaceChangeMarker {
        operation_id: crate::jj::jj_snapshot_head_operation_id(&workspace_path)
          .map_err(|error| format!("jj_command_failed: {error}"))?,
        working_copy_change_id: crate::jj::jj_get_change_id(&workspace_path, "@").ok(),
      }))
    }
    TreqCommandRequest::WorkspaceDiff { repo, workspace } => {
      let id = workspace_id(Some(&workspace))?.ok_or("invalid_arguments: workspace is required")?;
      json(crate::core::workspaces::workspace_diff(&repo, id))
    }
    TreqCommandRequest::CommitDiff {
      repo,
      workspace,
      revision,
    } => json(crate::core::commits::get_commit_diff_with_conflict_style(
      &repo,
      workspace_id(workspace.as_ref())?,
      &revision,
      "git",
    )),
    TreqCommandRequest::CommitFileDiff {
      repo,
      workspace,
      revision,
      path,
    } => json(
      crate::core::commits::get_commit_file_diff_with_conflict_style(
        &repo,
        workspace_id(workspace.as_ref())?,
        &revision,
        &path,
        "git",
      ),
    ),
    TreqCommandRequest::SearchFiles {
      repo,
      workspace,
      query,
      limit,
    } => json(search_files_local(
      &repo,
      workspace_id(workspace.as_ref())?,
      &query,
      limit.unwrap_or(50),
    )),
    TreqCommandRequest::ProbeRepo { repo } => json(probe_repo_path(&repo)),
    TreqCommandRequest::InitRepo {
      repo,
      idempotency_key,
    } => with_idempotency_key(
      &repo,
      "repo.init",
      Some(idempotency_key.as_str()),
      request_snapshot.as_ref().expect("InitRepo is a mutation"),
      || json(init_repo_path(&repo)),
    ),
    TreqCommandRequest::CloneRepo {
      repo_url,
      destination,
      idempotency_key,
    } => with_idempotency_key(
      // `destination` does not exist yet — cloning is the whole point —
      // so the idempotency store (which eagerly creates `<repo>/.treq/`
      // when opened) must not be scoped there or `git clone` will refuse
      // to write into an already-non-empty destination. Its parent
      // directory is guaranteed to exist since the caller chose a path
      // under it.
      Path::new(&destination)
        .parent()
        .and_then(|parent| parent.to_str())
        .unwrap_or("."),
      "repo.clone",
      Some(idempotency_key.as_str()),
      request_snapshot.as_ref().expect("CloneRepo is a mutation"),
      || json(clone_repo_local(&repo_url, &destination)),
    ),
    TreqCommandRequest::CreateWorkspace {
      repo,
      branch_name,
      source_branch,
      metadata,
      idempotency_key,
    } => with_idempotency_key(
      &repo,
      "workspace.create",
      Some(idempotency_key.as_str()),
      request_snapshot
        .as_ref()
        .expect("CreateWorkspace is a mutation"),
      || {
        json(create_workspace_from_metadata(
          &repo,
          &branch_name,
          source_branch.as_deref(),
          metadata.as_deref(),
        ))
      },
    ),
    TreqCommandRequest::RenameWorkspace {
      repo,
      workspace,
      new_name,
      idempotency_key,
    } => with_idempotency_key(
      &repo,
      "workspace.rename",
      Some(idempotency_key.as_str()),
      request_snapshot
        .as_ref()
        .expect("RenameWorkspace is a mutation"),
      || {
        let result = crate::core::workspaces::rename_workspace(
          &repo,
          workspace_id(Some(&workspace))?.ok_or("invalid_arguments: workspace is required")?,
          &new_name,
          false,
        )?;
        if !result.success {
          return Err(format!("invalid_arguments: {}", result.message));
        }
        json(Ok(result))
      },
    ),
    TreqCommandRequest::UpdateWorkspace {
      repo,
      workspace,
      target_branch,
      title,
      description,
    } => json(crate::core::workspaces::update_workspace_with_title(
      &repo,
      workspace_id(Some(&workspace))?.ok_or("invalid_arguments: workspace is required")?,
      target_branch.map_or(
        crate::core::workspaces::MaybeEmptyParam::Omitted,
        crate::core::workspaces::MaybeEmptyParam::Some,
      ),
      title.map_or(
        crate::core::workspaces::MaybeEmptyParam::Omitted,
        crate::core::workspaces::MaybeEmptyParam::Some,
      ),
      description.map_or(
        crate::core::workspaces::MaybeEmptyParam::Omitted,
        crate::core::workspaces::MaybeEmptyParam::Some,
      ),
    )),
    TreqCommandRequest::DeleteWorkspace { repo, workspace } => {
      let id = workspace_id(Some(&workspace))?.ok_or("invalid_arguments: workspace is required")?;
      json(crate::core::workspaces::delete_workspace(&repo, &id))
    }
    TreqCommandRequest::MoveWorkspaceChanges {
      repo,
      workspace,
      destination,
      files,
      hunks,
      commits,
      idempotency_key,
    } => with_idempotency_key(
      &repo,
      "workspace.move",
      Some(idempotency_key.as_str()),
      request_snapshot
        .as_ref()
        .expect("MoveWorkspaceChanges is a mutation"),
      || {
        let request = crate::core::workspaces::WorkspaceMoveRequest {
          files: files.into_iter().filter(|f| !f.is_empty()).collect(),
          hunks,
          commits: commits.into_iter().filter(|c| !c.is_empty()).collect(),
        };
        json(crate::core::workspaces::move_workspace_changes(
          &repo,
          &workspace,
          &destination,
          request,
        ))
      },
    ),
    TreqCommandRequest::RebaseWorkspace {
      repo,
      workspace,
      target_branch,
      idempotency_key,
    } => with_idempotency_key(
      &repo,
      "workspace.rebase",
      Some(idempotency_key.as_str()),
      request_snapshot
        .as_ref()
        .expect("RebaseWorkspace is a mutation"),
      || {
        let id =
          workspace_id(Some(&workspace))?.ok_or("invalid_arguments: workspace is required")?;
        // Stack planning needs the repository's real default branch; a
        // hardcoded "main" misplans retargets in repos that use another name.
        let default_branch = crate::jj::get_default_branch(&repo)
          .map_err(|error| format!("jj_command_failed: {error}"))?;
        json(crate::core::workspaces::retarget_workspace(
          &repo,
          id,
          &target_branch,
          &default_branch,
        ))
      },
    ),
    TreqCommandRequest::RestoreFile {
      repo,
      workspace,
      path,
    } => {
      let workspace_path = resolve_workspace_path(&repo, workspace_id(workspace.as_ref())?)?;
      json(crate::core::changes::discard_file_changes(
        &workspace_path,
        &path,
      ))
    }
    TreqCommandRequest::PatchFile {
      repo,
      workspace,
      path,
      patch_base64,
      idempotency_key,
    } => with_idempotency_key(
      &repo,
      "file.patch",
      Some(idempotency_key.as_str()),
      request_snapshot.as_ref().expect("PatchFile is a mutation"),
      || {
        json(apply_remote_patch(
          &repo,
          workspace_id(workspace.as_ref())?,
          &path,
          &patch_base64,
        ))
      },
    ),
    TreqCommandRequest::CreateCommit {
      repo,
      workspace,
      message,
      idempotency_key,
      ..
    } => with_idempotency_key(
      &repo,
      "commit.create",
      Some(idempotency_key.as_str()),
      request_snapshot
        .as_ref()
        .expect("CreateCommit is a mutation"),
      || {
        if message.trim().is_empty() {
          return Err("invalid_arguments: commit message must not be blank".to_string());
        }
        let id = workspace_id(workspace.as_ref())?;
        let committed =
          crate::core::workspaces::commit_workspace_changes_with_auto_push(&repo, id, &message)
            .map_err(|e| match e.as_str() {
              crate::core::NOTHING_TO_COMMIT => format!("invalid_arguments: {e}"),
              _ => e,
            });
        json(committed)
      },
    ),
    TreqCommandRequest::DescribeCommit {
      repo,
      workspace,
      commit,
      message,
    } => {
      let id = workspace_id(Some(&workspace))?.ok_or("invalid_arguments: workspace is required")?;
      // Naturally idempotent: it overwrites the description each call, so no
      // idempotency key is needed or accepted.
      json(crate::core::commits::describe_commit(
        &repo, id, &commit, &message,
      ))
    }
    TreqCommandRequest::SplitCommit {
      repo,
      workspace,
      commit,
      files,
      hunks,
      message,
      idempotency_key,
    } => {
      // The underlying split works on whole files of the working copy.
      // Hunk selections would silently become whole-file splits, so they are
      // refused before the idempotency store records a claim.
      if !hunks.is_empty() {
        return Err(
          "unsupported: SplitCommit does not support hunk selections yet; select whole files"
            .to_string(),
        );
      }
      with_idempotency_key(
        &repo,
        "commit.split",
        Some(idempotency_key.as_str()),
        request_snapshot
          .as_ref()
          .expect("SplitCommit is a mutation"),
        || {
          let selected: Vec<String> = files
            .into_iter()
            .map(|f| f.trim().to_string())
            .filter(|f| !f.is_empty())
            .collect();
          if selected.is_empty() {
            return Err(
              "invalid_arguments: SplitCommit requires selected files or hunks".to_string(),
            );
          }
          let id =
            workspace_id(Some(&workspace))?.ok_or("invalid_arguments: workspace is required")?;
          let workspace_path = resolve_workspace_path(&repo, Some(id))?;
          ensure_working_copy_revision(&workspace_path, &commit)?;
          json(
            crate::jj::jj_split(&workspace_path, &message, selected)
              .map_err(|error| format!("jj_command_failed: {error}")),
          )
        },
      )
    }
    TreqCommandRequest::MoveCommit {
      repo,
      workspace,
      commit,
      target_workspace,
      idempotency_key,
    } => with_idempotency_key(
      &repo,
      "commit.move",
      Some(idempotency_key.as_str()),
      request_snapshot.as_ref().expect("MoveCommit is a mutation"),
      || {
        let source_id =
          workspace_id(Some(&workspace))?.ok_or("invalid_arguments: workspace is required")?;
        let target_id = workspace_id(Some(&target_workspace))?
          .ok_or("invalid_arguments: target workspace is required")?;
        json(crate::core::commits::move_commit_to_existing_workspace(
          &repo, source_id, &commit, target_id,
        ))
      },
    ),
    TreqCommandRequest::AbandonCommit {
      repo,
      workspace,
      commit,
      idempotency_key,
    } => with_idempotency_key(
      &repo,
      "commit.abandon",
      Some(idempotency_key.as_str()),
      request_snapshot
        .as_ref()
        .expect("AbandonCommit is a mutation"),
      || {
        let id =
          workspace_id(Some(&workspace))?.ok_or("invalid_arguments: workspace is required")?;
        json(crate::core::commits::abandon_commit(&repo, id, &commit))
      },
    ),
    TreqCommandRequest::ResolveConflict {
      repo,
      revision,
      sides,
      idempotency_key,
    } => with_idempotency_key(
      &repo,
      "conflict.resolve",
      Some(idempotency_key.as_str()),
      request_snapshot
        .as_ref()
        .expect("ResolveConflict is a mutation"),
      || {
        let sides = crate::core::resolve::parse_resolve_sides(
          &sides
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>(),
        )?;
        json(crate::core::resolve::resolve_commit(
          &repo, &revision, &sides, None,
        ))
      },
    ),
    TreqCommandRequest::GitFetch { repo } => {
      json(crate::jj::jj_git_fetch(&repo).map_err(|error| format!("jj_command_failed: {error}")))
    }
    TreqCommandRequest::GitBookmarkTrack {
      repo,
      bookmark,
      remote_name,
    } => {
      // Tracking an already-tracked bookmark is a no-op in jj, so this is
      // naturally idempotent and does not need a key.
      json(
        crate::jj::jj_bookmark_track(&repo, &bookmark, &remote_name)
          .map_err(|error| format!("jj_command_failed: {error}")),
      )
    }
    TreqCommandRequest::GitPush {
      repo,
      workspace,
      idempotency_key,
    } => with_idempotency_key(
      &repo,
      "git.push",
      Some(idempotency_key.as_str()),
      request_snapshot.as_ref().expect("GitPush is a mutation"),
      || {
        json(crate::core::workspaces::push_workspace_to_remote(
          &repo,
          workspace_id(workspace.as_ref())?,
        ))
      },
    ),
    TreqCommandRequest::AgentStart {
      repo,
      workspace,
      agent,
      prompt,
      idempotency_key,
    } => with_idempotency_key(
      &repo,
      "agent.start",
      Some(idempotency_key.as_str()),
      request_snapshot.as_ref().expect("AgentStart is a mutation"),
      || {
        let id =
          workspace_id(Some(&workspace))?.ok_or("invalid_arguments: workspace is required")?;
        let workspace_path = resolve_workspace_path(&repo, Some(id))?;
        json(crate::core::agent_supervisor::start_agent(
          &repo,
          &workspace,
          &workspace_path,
          &agent,
          &prompt,
        ))
      },
    ),
    TreqCommandRequest::AgentInput {
      repo,
      workspace,
      input,
      idempotency_key,
    } => with_idempotency_key(
      &repo,
      "agent.input",
      Some(idempotency_key.as_str()),
      request_snapshot.as_ref().expect("AgentInput is a mutation"),
      || {
        json(crate::core::agent_supervisor::send_agent_input(
          &repo, &workspace, &input,
        ))
      },
    ),
    TreqCommandRequest::AgentStatus { repo, workspace } => json(
      crate::core::agent_supervisor::agent_status(&repo, &workspace),
    ),
    TreqCommandRequest::AgentStop { repo, workspace } => {
      json(crate::core::agent_supervisor::stop_agent(&repo, &workspace))
    }
    TreqCommandRequest::AgentLogs { repo, workspace } => {
      json(crate::core::agent_supervisor::agent_logs(&repo, &workspace))
    }
    TreqCommandRequest::PtyStart {
      repo,
      workspace,
      label,
      remote_dir,
      launch,
      cols,
      rows,
      idempotency_key,
    } => with_idempotency_key(
      &repo,
      "pty.start",
      Some(idempotency_key.as_str()),
      request_snapshot.as_ref().expect("PtyStart is a mutation"),
      || {
        json(crate::core::pty_remote_supervisor::start_session(
          &repo,
          &remote_dir,
          &workspace,
          &label,
          &launch,
          cols,
          rows,
        ))
      },
    ),
    TreqCommandRequest::PtyList { repo, workspace } => json(
      crate::core::pty_remote_supervisor::list_sessions(&repo, workspace.as_deref()),
    ),
    TreqCommandRequest::PtyStop {
      repo,
      workspace,
      label,
    } => json(crate::core::pty_remote_supervisor::stop_session(
      &repo, &workspace, &label,
    )),
    TreqCommandRequest::PtyAttachCommand {
      repo,
      workspace,
      label,
      remote_dir,
      launch,
      cols,
      rows,
    } => json(crate::core::pty_remote_supervisor::build_attach_command(
      &repo,
      &remote_dir,
      &workspace,
      &label,
      &launch,
      cols,
      rows,
    )),
  }
}

/// Mirrors the local `create_workspace` Tauri command: parses the same
/// metadata JSON, applies the title, and marks the workspace for its first
/// rebase. Repository settings that live in the desktop app database (copied
/// files, setup scripts) are not available on the VM and are skipped.
fn create_workspace_from_metadata(
  repo: &str,
  branch_name: &str,
  source_branch: Option<&str>,
  metadata: Option<&str>,
) -> Result<crate::local_db::Workspace, String> {
  let parsed = crate::core::workspaces::parse_workspace_metadata(metadata);
  let workspace = crate::core::workspaces::create_workspace_with_symlinked_dirs(
    repo,
    branch_name,
    parsed.description,
    parsed.moved_files,
    source_branch,
    None,
    parsed.sparse_patterns,
    parsed.symlinked_dirs,
  )?;
  if let Some(title) = parsed.title {
    crate::local_db::update_workspace_title(repo, workspace.id, &title)?;
  }
  crate::local_db::update_workspace_last_rebased_commit(repo, workspace.id, "")?;
  crate::local_db::get_workspace_by_id(repo, workspace.id)?.ok_or_else(|| {
    format!(
      "workspace_not_found: Workspace {} was not found",
      workspace.id
    )
  })
}

/// Split only runs against the working copy. Accept `@` or the working
/// copy's own change id, and refuse anything else so a request for another
/// commit is never applied to `@` by mistake.
fn ensure_working_copy_revision(workspace_path: &str, commit: &str) -> Result<(), String> {
  let commit = commit.trim();
  if commit.is_empty() || commit == "@" {
    return Ok(());
  }
  let working_copy = crate::jj::jj_get_change_id(workspace_path, "@")
    .map_err(|error| format!("jj_command_failed: {error}"))?;
  if change_ids_match(&working_copy, commit) {
    Ok(())
  } else {
    Err(format!(
      "unsupported: SplitCommit can only split the working copy, not {commit}"
    ))
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteFileSearchResult {
  pub file_path: String,
  pub relative_path: String,
}

/// Searches the workspace file index. Each remote CLI call is a fresh
/// process, so the index may not exist yet on the VM; an empty result
/// triggers one indexing pass and a second search.
fn search_files_local(
  repo: &str,
  workspace: Option<i64>,
  query: &str,
  limit: usize,
) -> Result<Vec<RemoteFileSearchResult>, String> {
  let search = || crate::local_db::search_workspace_files(repo, workspace, query, limit);
  let mut files = search()?;
  if files.is_empty() {
    let workspace_path = resolve_workspace_path(repo, workspace)?;
    crate::file_indexer::index_workspace_files(repo, workspace, &workspace_path)?;
    files = search()?;
  }
  Ok(
    files
      .into_iter()
      .map(|f| RemoteFileSearchResult {
        file_path: f.file_path,
        relative_path: f.relative_path,
      })
      .collect(),
  )
}

fn probe_repo_path(repo_path: &str) -> Result<RemoteRepoProbe, String> {
  let trimmed = repo_path.trim();
  if trimmed.is_empty() {
    return Err("Remote path is required".to_string());
  }
  let path = Path::new(trimmed);
  let exists = path.exists();
  let is_repo = exists && (path.join(".jj").is_dir() || path.join(".git").exists());
  Ok(RemoteRepoProbe {
    host: String::new(),
    path: trimmed.to_string(),
    exists,
    is_repo,
    needs_clone: !is_repo,
  })
}

/// Recursively sums file sizes under `root` (best effort: unreadable entries
/// are skipped rather than failing the whole walk) as the "disk used"
/// figure for base-disk-quota enforcement. This walks the repository root
/// rather than statvfs-ing the whole volume, since the quota is scoped to
/// what the user's repositories occupy, not the base image itself.
fn directory_usage_bytes(root: &Path) -> u64 {
  let mut total = 0u64;
  let mut stack = vec![root.to_path_buf()];
  while let Some(dir) = stack.pop() {
    let Ok(entries) = fs::read_dir(&dir) else {
      continue;
    };
    for entry in entries.flatten() {
      let Ok(metadata) = entry.metadata() else {
        continue;
      };
      if metadata.is_dir() {
        stack.push(entry.path());
      } else {
        total += metadata.len();
      }
    }
  }
  total
}

/// Machine-wide usage returned by [`TreqCommandRequest::MachineUsage`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineUsageReport {
  pub repository_count: usize,
  pub workspace_count: usize,
  pub disk_used_bytes: u64,
}

/// Repositories sit at most two levels below the root
/// (`<root>/<owner>/<name>` for managed clones, `<root>/<name>` otherwise).
const MACHINE_USAGE_REPO_DEPTH: usize = 2;

fn is_repository_dir(path: &Path) -> bool {
  path.join(".jj").is_dir() || path.join(".git").exists()
}

fn find_repositories(root: &Path) -> Vec<PathBuf> {
  let mut repositories = Vec::new();
  let mut stack = vec![(root.to_path_buf(), 0usize)];
  while let Some((dir, depth)) = stack.pop() {
    if depth > 0 && is_repository_dir(&dir) {
      repositories.push(dir);
      continue;
    }
    if depth == MACHINE_USAGE_REPO_DEPTH {
      continue;
    }
    let Ok(entries) = fs::read_dir(&dir) else {
      continue;
    };
    for entry in entries.flatten() {
      if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
        stack.push((entry.path(), depth + 1));
      }
    }
  }
  repositories
}

/// Counts repositories and their Treq workspaces under `root` and measures
/// the bytes they use. A missing root reports zero usage rather than an
/// error, since a fresh machine has no repositories directory yet.
fn machine_usage(root: &str) -> Result<MachineUsageReport, String> {
  validate_remote_path(root)?;
  let root = Path::new(root.trim());
  if !root.is_dir() {
    return Ok(MachineUsageReport::default());
  }
  let repositories = find_repositories(root);
  let workspace_count = repositories
    .iter()
    .filter_map(|repo| repo.to_str())
    .map(|repo| {
      crate::core::workspaces::list_workspaces(repo)
        .map(|workspaces| workspaces.len())
        .unwrap_or(0)
    })
    .sum();
  Ok(MachineUsageReport {
    repository_count: repositories.len(),
    workspace_count,
    disk_used_bytes: directory_usage_bytes(root),
  })
}

/// Enforces the base disk quota (PRD "Resource quotas") before a
/// write-triggering mutation (repo init, clone, patch apply) proceeds.
/// Returns the distinct `disk_quota_exceeded: ...` structured error the PRD
/// requires instead of letting the write fail as a generic filesystem
/// error. `root` is the directory whose usage counts against the quota —
/// callers pass the repository root (or its parent, before it exists) so
/// the check reflects what the write is about to add to.
fn enforce_disk_quota(root: &Path) -> Result<(), String> {
  enforce_disk_quota_against(root, crate::core::remote_provider::BASE_DISK_QUOTA_BYTES)
}

/// Same as [`enforce_disk_quota`] but against an arbitrary `quota_bytes`
/// ceiling, so tests can exercise the failure path without writing 5 GB of
/// fixture data to disk.
fn enforce_disk_quota_against(root: &Path, quota_bytes: u64) -> Result<(), String> {
  let scan_root = if root.exists() {
    root.to_path_buf()
  } else {
    root
      .parent()
      .map(Path::to_path_buf)
      .unwrap_or_else(|| root.to_path_buf())
  };
  let used_bytes = directory_usage_bytes(&scan_root);
  crate::core::remote_provider::check_disk_quota_against(used_bytes, quota_bytes)
    .map_err(|err| err.to_string())
}

fn init_repo_path(repo_path: &str) -> Result<RepositoryInspection, String> {
  let trimmed = repo_path.trim();
  if trimmed.is_empty() {
    return Err("Remote path is required".to_string());
  }
  let path = Path::new(trimmed);
  enforce_disk_quota(path)?;
  fs::create_dir_all(path)
    .map_err(|e| format!("filesystem_error: Failed to create {trimmed}: {e}"))?;
  if !path.join(".jj").is_dir() && !path.join(".git").exists() {
    jj::jj_git_init_colocated(trimmed)
      .map_err(|e| format!("jj_command_failed: Failed to initialize repository: {e}"))?;
  }
  inspect_repository_path(trimmed)
}

fn clone_repo_local(repo_url: &str, destination: &str) -> Result<RepositoryInspection, String> {
  if repo_url.trim().is_empty() {
    return Err("Repository URL is required".to_string());
  }
  validate_remote_path(destination)?;
  enforce_disk_quota(Path::new(destination))?;
  let interrupt = std::sync::atomic::AtomicBool::new(false);
  let clone_err = |e: &dyn std::fmt::Display| format!("git_command_failed: {e}");
  // Dropping an unfinished clone removes the partial destination.
  let mut prepare = gix::prepare_clone(repo_url, destination).map_err(|e| clone_err(&e))?;
  let (mut checkout, _) = prepare
    .fetch_then_checkout(gix::progress::Discard, &interrupt)
    .map_err(|e| clone_err(&e))?;
  // A remote HEAD naming a missing branch leaves nothing to check out. An
  // empty remote (no branches at all) still clones, as `git clone` does.
  let repo = checkout.repo();
  let head = repo.head().map_err(|e| clone_err(&e))?;
  let has_branches = repo
    .references()
    .map_err(|e| clone_err(&e))?
    .prefixed("refs/remotes/")
    .map_err(|e| clone_err(&e))?
    .filter_map(Result::ok)
    .any(|r| !r.name().as_bstr().ends_with(b"/HEAD"));
  if head.is_unborn() && has_branches {
    let branch = head
      .referent_name()
      .map(|name| name.shorten().to_string())
      .unwrap_or_default();
    return Err(format!(
      "git_command_failed: remote HEAD points at branch '{branch}', which does not exist"
    ));
  }
  checkout
    .main_worktree(gix::progress::Discard, &interrupt)
    .map_err(|e| clone_err(&e))?;
  inspect_repository_path(destination)
}

/// Applies a base64-encoded unified diff to `path` inside the given
/// workspace (or the repo root) using `git apply`. Base64 keeps the exec
/// argument vector a plain string even though the payload contains newlines.
fn require_existing_repo(repo: &str) -> Result<(), String> {
  inspect_repository_path(repo).map(|_| ())
}

/// Applies a base64-encoded unified diff to `path` inside the given
/// workspace (or the repo root). Base64 keeps the exec argument vector a plain
/// string even though the payload contains newlines.
fn apply_remote_patch(
  repo: &str,
  workspace: Option<i64>,
  path: &str,
  patch_base64: &str,
) -> Result<String, String> {
  let workspace_path = resolve_workspace_path(repo, workspace)?;
  enforce_disk_quota(Path::new(&workspace_path))?;
  let decoded = decode_base64(patch_base64.trim())
    .map_err(|e| format!("invalid_arguments: patch is not valid base64: {e}"))?;
  let patch = String::from_utf8(decoded)
    .map_err(|_| "invalid_arguments: patch is not UTF-8 text".to_string())?;
  jj::jj_apply_file_patch(&workspace_path, path, &patch).map_err(|e| match e {
    jj::JjError::IoError(message) if message.starts_with("Invalid patch:") => {
      format!("invalid_arguments: {message}")
    }
    other => format!("filesystem_error: {other}"),
  })?;
  Ok(format!("Applied patch to {path}"))
}

/// Minimal RFC 4648 base64 decoder (standard alphabet, `=` padding) so patch
/// payloads can travel through the plain-string exec argument vector without
/// pulling in an extra crate dependency.
fn decode_base64(input: &str) -> Result<Vec<u8>, String> {
  fn value(byte: u8) -> Option<u8> {
    match byte {
      b'A'..=b'Z' => Some(byte - b'A'),
      b'a'..=b'z' => Some(byte - b'a' + 26),
      b'0'..=b'9' => Some(byte - b'0' + 52),
      b'+' => Some(62),
      b'/' => Some(63),
      _ => None,
    }
  }
  let cleaned: Vec<u8> = input.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
  let mut out = Vec::with_capacity(cleaned.len() / 4 * 3);
  for chunk in cleaned.chunks(4) {
    if chunk.len() < 2 {
      return Err("truncated base64 input".to_string());
    }
    let padding = chunk.iter().filter(|&&b| b == b'=').count();
    let vals: Vec<u8> = chunk
      .iter()
      .filter(|&&b| b != b'=')
      .map(|&b| value(b).ok_or_else(|| "invalid base64 character".to_string()))
      .collect::<Result<_, _>>()?;
    let mut buf = [0u8; 4];
    for (i, v) in vals.iter().enumerate() {
      buf[i] = *v;
    }
    out.push((buf[0] << 2) | (buf[1] >> 4));
    if padding < 2 && vals.len() > 2 {
      out.push((buf[1] << 4) | (buf[2] >> 2));
    }
    if padding < 1 && vals.len() > 3 {
      out.push((buf[2] << 6) | buf[3]);
    }
  }
  Ok(out)
}

// ---------------------------------------------------------------------------
// Native (Phase 4) SSH exec transport for typed commands
// ---------------------------------------------------------------------------
//
// Every reachable connection path, managed or user-managed, goes through the
// pooled native `russh` transport below and in
// `crate::core::remote_ssh_transport`, never a system `ssh` subprocess.

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

pub fn inspect_repository_path(repo_path: &str) -> Result<RepositoryInspection, String> {
  let root = validate_repository_path(repo_path)?;
  let repository_type = detect_repository_type(&root)?;
  let branch = crate::core::repo::get_repo_current_branch(&root).ok();
  let default_branch =
    crate::core::repo::get_repo_default_branch(&root).unwrap_or_else(|_| "main".to_string());
  let current_commit_id = jj::jj_get_commit_id(&root, "@").unwrap_or_default();
  let current_change_id = String::new();
  let display_name = repository_display_name(&root);

  Ok(RepositoryInspection {
    root: root.clone(),
    repository_type,
    current_branch: branch.and_then(|branch| branch.current_branch),
    default_branch,
    current_change_id,
    current_commit_id,
    descriptor: RepositoryDescriptor {
      id: format!("local:{root}"),
      location: RepositoryLocation::Local { path: root },
      display_name,
    },
  })
}

pub fn parse_ssh_config_hosts(contents: &str) -> Vec<SshHost> {
  let mut aliases = BTreeSet::new();
  for line in contents.lines() {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
      continue;
    }
    let mut parts = trimmed.split_whitespace();
    let Some(keyword) = parts.next() else {
      continue;
    };
    if !keyword.eq_ignore_ascii_case("host") {
      continue;
    }
    for alias in parts {
      if alias == "*" || alias.contains('*') || alias.contains('?') || alias.starts_with('!') {
        continue;
      }
      aliases.insert(alias.to_string());
    }
  }
  aliases.into_iter().map(|alias| SshHost { alias }).collect()
}

pub fn list_configured_hosts() -> Result<Vec<SshHost>, String> {
  list_configured_hosts_from_paths(ssh_config_paths())
}

pub fn list_configured_hosts_from_paths(paths: Vec<PathBuf>) -> Result<Vec<SshHost>, String> {
  let mut all = BTreeSet::new();
  for path in paths {
    if let Ok(contents) = fs::read_to_string(path) {
      for host in parse_ssh_config_hosts(&contents) {
        all.insert(host.alias);
      }
    }
  }
  Ok(all.into_iter().map(|alias| SshHost { alias }).collect())
}

pub fn build_ssh_shell_command(
  host: &str,
  working_dir: Option<&str>,
  initial_command: Option<&str>,
) -> Result<(String, Vec<String>), String> {
  validate_host_alias(host)?;
  let mut remote_command = String::new();
  if let Some(dir) = working_dir {
    validate_remote_path(dir)?;
    remote_command.push_str("cd ");
    remote_command.push_str(&shell_quote(dir));
    remote_command.push_str(" && ");
  }
  remote_command.push_str("${SHELL:-/bin/sh} -l");
  if let Some(command) = initial_command {
    if !command.trim().is_empty() {
      remote_command.push_str(" -c ");
      remote_command.push_str(&shell_quote(command));
    }
  }
  Ok((
    "ssh".to_string(),
    vec![host.to_string(), "-t".to_string(), remote_command],
  ))
}

fn validate_repository_path(repo_path: &str) -> Result<String, String> {
  let trimmed = repo_path.trim();
  if trimmed.is_empty() {
    return Err("Repository path is required".to_string());
  }
  let path = Path::new(trimmed);
  if !path.exists() {
    return Err("repository_not_found: Repository path does not exist".to_string());
  }
  if !path.is_dir() {
    return Err("invalid_repository: Repository path is not a directory".to_string());
  }
  std::fs::canonicalize(path)
    .map_err(|e| format!("permission_denied: Failed to read repository path: {e}"))?
    .to_str()
    .map(|path| path.to_string())
    .ok_or_else(|| "invalid_repository: Repository path is not valid UTF-8".to_string())
}

fn detect_repository_type(root: &str) -> Result<String, String> {
  let path = Path::new(root);
  let has_jj = path.join(".jj").is_dir();
  let has_git = path.join(".git").exists();
  match (has_jj, has_git) {
    (true, true) => Ok("jj_colocated".to_string()),
    (true, false) => Ok("jj".to_string()),
    (false, true) => Ok("git".to_string()),
    (false, false) => Err("invalid_repository: No .jj or .git directory found".to_string()),
  }
}

fn repository_display_name(path: &str) -> String {
  Path::new(path)
    .file_name()
    .and_then(|name| name.to_str())
    .filter(|name| !name.is_empty())
    .unwrap_or(path)
    .to_string()
}

fn ssh_config_paths() -> Vec<PathBuf> {
  let mut paths = Vec::new();
  if let Ok(home) = std::env::var("HOME") {
    paths.push(PathBuf::from(home).join(".ssh").join("config"));
  }
  paths.push(PathBuf::from("/etc/ssh/ssh_config"));
  paths
}

pub fn shell_quote(value: &str) -> String {
  format!("'{}'", value.replace('\'', "'\\''"))
}

fn validate_host_alias(host: &str) -> Result<(), String> {
  if host.trim().is_empty() {
    return Err("SSH host is required".to_string());
  }
  if host
    .chars()
    .any(|c| c.is_whitespace() || matches!(c, ';' | '&' | '|' | '`' | '$' | '<' | '>'))
  {
    return Err("SSH host must be a host alias from ssh config".to_string());
  }
  Ok(())
}

fn validate_remote_path(path: &str) -> Result<(), String> {
  if path.trim().is_empty() {
    return Err("Remote path is required".to_string());
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::process::Command;

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
    assert_eq!(args[0], "devbox");
    assert!(args[2].contains("cd '/srv/my app'"));
  }

  #[test]
  fn rejects_unsafe_host_aliases() {
    assert!(build_ssh_shell_command("dev; rm -rf /", None, None).is_err());
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
      repo: repo_path.clone(),
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
  fn kind_names_match_serde_kind_tags() {
    let sample = TreqCommandRequest::GitFetch { repo: "/r".into() };
    let value = serde_json::to_value(&sample).unwrap();
    assert_eq!(value["kind"], "GitFetch");
    assert!(TreqCommandRequest::KIND_NAMES.contains(&sample.kind_name()));
    assert_eq!(TreqCommandRequest::KIND_NAMES.len(), 45);
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

  // -- Real repository round trips (need the tauri-test helpers) ---------------

  #[cfg(feature = "tauri-test")]
  mod real_repo {
    use super::super::*;
    use crate::e2e_test_helpers::TestRepo;

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
        .clone()
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
        repo: repo.repo_path.clone(),
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
