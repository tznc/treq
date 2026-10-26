use super::{
  apply_remote_patch, change_ids_match, clone_repo_local, clone_request, create_pull_request,
  github_read, github_write, host_check, init_repo_path, inspect_repository_path, list_directories,
  list_workspace_files, machine_usage, probe_repo_path, require_existing_repo, verification_for,
  workspace_pull_request, Deserialize, DeserializeOwned, FileRevision, MutationVerificationOutcome,
  Path, ResultExt, ReviewScope, Serialize, TransportErrorExt, TreqCommandRequest,
  WorkspaceChangeMarker,
};

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
pub(super) fn with_idempotency_key<F>(
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
  store
    .cleanup(crate::core::idempotency_store::DEFAULT_RETENTION)
    .warn_on_err("cleanup");
  outcome
}

/// Resolves a stale pending claim by running the mutation's existing
/// observable-state verification recipe locally (the same recipe
/// [`verification_for`] builds for the async post-reconnect retry path,
/// applied here via a synchronous local dispatch instead of an SSH read).
pub(super) fn recover_stale_claim(
  request: &TreqCommandRequest,
) -> Result<RecoveryDecision, String> {
  let Some(verification) = verification_for(request) else {
    return Ok(RecoveryDecision::Ambiguous);
  };
  let Ok(observed) = execute_local_request(verification.read_request) else {
    return Ok(RecoveryDecision::Ambiguous);
  };
  match (verification.check)(&observed) {
    MutationVerificationOutcome::AlreadyApplied => {
      Ok(RecoveryDecision::AlreadyApplied { observed })
    }
    MutationVerificationOutcome::NotApplied => Ok(RecoveryDecision::VerifiedNotApplied),
    MutationVerificationOutcome::Ambiguous => Ok(RecoveryDecision::Ambiguous),
  }
}

pub(super) fn resolve_workspace_path(repo: &str, id: Option<i64>) -> Result<String, String> {
  match id {
    None => Ok(repo.to_string()),
    Some(id) => {
      let workspace = crate::local_db::get_workspace_by_id(repo, id)?
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

pub(super) fn workspace_id(value: Option<&String>) -> Result<Option<i64>, String> {
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
#[expect(
  clippy::expect_used,
  reason = "every mutation request carries a snapshot"
)]
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
    TreqCommandRequest::WorkspaceSummaries { repo } => {
      require_existing_repo(&repo)?;
      json(crate::core::workspaces::workspace_summaries(&repo))
    }
    TreqCommandRequest::PlanNewWorkspaces {
      repo,
      text,
      stack_on,
    } => {
      require_existing_repo(&repo)?;
      json(crate::core::new_workspaces::plan(
        &repo,
        &text,
        stack_on.as_deref(),
      ))
    }
    TreqCommandRequest::MachineUsage { root } => json(machine_usage(&root)),
    TreqCommandRequest::MachineTools { .. } => json(Ok::<_, String>(
      crate::core::prerequisites::check_host_tools(),
    )),
    TreqCommandRequest::ListDirectories { path } => json(list_directories(&path)),
    TreqCommandRequest::HostCheck { root } => json(Ok::<_, String>(host_check(&root))),
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
    // The marker style `WorkspaceDiff` names its token with on a host.
    TreqCommandRequest::DiffFiles {
      repo,
      workspace,
      paths,
      token,
    } => json(crate::core::changes::list_file_hunks_batch(
      &repo,
      workspace_id(workspace.as_ref())?,
      paths,
      crate::core::DEFAULT_CONFLICT_MARKER_STYLE,
      token.as_deref(),
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
      // A host has no app settings database, so the diff takes the default
      // conflict marker style, as `treq changes workspace-diff` does.
      json(crate::core::workspaces::workspace_cli_diff(&repo, id))
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
    TreqCommandRequest::ListFiles {
      repo,
      workspace,
      path,
    } => json(list_workspace_files(
      &repo,
      workspace_id(workspace.as_ref())?,
      &path,
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
    TreqCommandRequest::NewWorkspaces {
      repo,
      text,
      stack_on,
      idempotency_key,
    } => with_idempotency_key(
      &repo,
      "workspace.new",
      Some(idempotency_key.as_str()),
      request_snapshot
        .as_ref()
        .expect("NewWorkspaces is a mutation"),
      || {
        require_existing_repo(&repo)?;
        json(crate::core::new_workspaces::run(
          &repo,
          &text,
          stack_on.as_deref(),
        ))
      },
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
    TreqCommandRequest::ArchiveWorkspace { repo, workspace } => {
      let id = workspace_id(Some(&workspace))?.ok_or("invalid_arguments: workspace is required")?;
      json(crate::core::workspaces::archive_workspace(&repo, &id))
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
    TreqCommandRequest::SwitchRepoBranch { repo, bookmark } => {
      json(crate::core::repo::switch_repo_branch(&repo, &bookmark))
    }
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
    TreqCommandRequest::SnapshotWorkingCopy { repo, workspace } => {
      json(crate::core::snapshot_working_copy(&resolve_workspace_path(
        &repo,
        workspace_id(workspace.as_ref())?,
      )?))
    }
    TreqCommandRequest::RestoreAll { repo, workspace } => json(crate::core::discard_all_changes(
      &resolve_workspace_path(&repo, workspace_id(workspace.as_ref())?)?,
    )),
    TreqCommandRequest::RestoreSnapshot {
      repo,
      workspace,
      snapshot_id,
    } => json(crate::core::restore_working_copy_snapshot(
      &resolve_workspace_path(&repo, workspace_id(workspace.as_ref())?)?,
      &snapshot_id,
    )),
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
    TreqCommandRequest::StashWorkspaceChanges {
      repo,
      workspace,
      idempotency_key,
    } => with_idempotency_key(
      &repo,
      "stash.workspace",
      Some(idempotency_key.as_str()),
      request_snapshot.as_ref().expect("stash is a mutation"),
      || {
        json(crate::core::stash::stash_workspace_changes(
          &repo,
          workspace_id(workspace.as_ref())?,
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
        &repo,
        Some(id),
        &commit,
        &message,
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
    TreqCommandRequest::UndoCommit {
      repo,
      workspace,
      commit,
    } => {
      let id = workspace_id(Some(&workspace))?.ok_or("invalid_arguments: workspace is required")?;
      json(crate::core::commits::undo_commit(&repo, id, &commit))
    }
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
        json(crate::core::commits::abandon_commit(
          &repo,
          Some(id),
          &commit,
        ))
      },
    ),
    TreqCommandRequest::UndoOperation {
      repo,
      workspace,
      operation_id,
    } => json(crate::core::commits::undo_repo_operation(
      &repo,
      workspace_id(workspace.as_ref())?,
      &operation_id,
    )),
    TreqCommandRequest::RevertCommit {
      repo,
      workspace,
      commit,
      idempotency_key,
    } => with_idempotency_key(
      &repo,
      "commit.revert",
      Some(idempotency_key.as_str()),
      request_snapshot
        .as_ref()
        .expect("RevertCommit is a mutation"),
      || {
        let id =
          workspace_id(Some(&workspace))?.ok_or("invalid_arguments: workspace is required")?;
        json(crate::core::commits::revert_commit(
          &repo,
          Some(id),
          &commit,
        ))
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
    // The desktop conflict-marker setting lives in the client's app
    // database, which the VM does not have, so pulls use the default style.
    TreqCommandRequest::PullWorkspace { repo, workspace } => {
      json(crate::core::workspaces::pull_workspace_from_remote(
        &repo,
        workspace_id(workspace.as_ref())?,
        crate::core::DEFAULT_CONFLICT_MARKER_STYLE,
      ))
    }
    TreqCommandRequest::GitRemoteInfo { repo } => {
      require_existing_repo(&repo)?;
      json(crate::github::get_git_remote_url_impl(&repo))
    }
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
    TreqCommandRequest::WorkspacePullRequest { repo, workspace } => json(workspace_pull_request(
      &repo,
      workspace_id(Some(&workspace))?.ok_or("invalid_arguments: workspace is required")?,
    )),
    TreqCommandRequest::CreatePullRequest {
      repo,
      workspace,
      title,
      body,
      draft,
      idempotency_key,
    } => with_idempotency_key(
      &repo,
      "git.pr-create",
      Some(idempotency_key.as_str()),
      request_snapshot
        .as_ref()
        .expect("CreatePullRequest is a mutation"),
      || {
        json(create_pull_request(
          &repo,
          workspace_id(Some(&workspace))?.ok_or("invalid_arguments: workspace is required")?,
          &title,
          &body,
          draft,
        ))
      },
    ),
    TreqCommandRequest::GithubRead { repo, query } => github_read(&repo, query),
    TreqCommandRequest::GithubWrite {
      repo,
      action,
      idempotency_key,
    } => with_idempotency_key(
      &repo,
      "git.github-write",
      Some(idempotency_key.as_str()),
      request_snapshot
        .as_ref()
        .expect("GithubWrite is a mutation"),
      || github_write(&repo, action),
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
      backend,
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
          backend,
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
    TreqCommandRequest::PtyList {
      backend,
      repo,
      workspace,
    } => json(crate::core::pty_remote_supervisor::list_sessions(
      backend,
      &repo,
      workspace.as_deref(),
    )),
    TreqCommandRequest::PtyStatus => json(crate::core::pty_remote_supervisor::terminal_status()),
    TreqCommandRequest::PtyReset { backend } => {
      json(crate::core::pty_remote_supervisor::reset_sessions(backend))
    }
    TreqCommandRequest::PtyStop {
      backend,
      repo,
      workspace,
      label,
    } => json(crate::core::pty_remote_supervisor::stop_session(
      backend, &repo, &workspace, &label,
    )),
    TreqCommandRequest::PtyRename {
      backend,
      repo,
      workspace,
      label,
      title,
    } => json(crate::core::pty_remote_supervisor::rename_session(
      backend, &repo, &workspace, &label, &title,
    )),
    TreqCommandRequest::PtyAttachCommand {
      backend,
      repo,
      workspace,
      label,
      remote_dir,
      launch,
      cols,
      rows,
    } => json(crate::core::pty_remote_supervisor::build_attach_command(
      backend,
      &repo,
      &remote_dir,
      &workspace,
      &label,
      &launch,
      cols,
      rows,
    )),
    TreqCommandRequest::PtyPaste {
      backend,
      repo,
      workspace,
      label,
      text,
      submit,
      idempotency_key,
    } => with_idempotency_key(
      &repo,
      "pty.paste",
      Some(idempotency_key.as_str()),
      request_snapshot.as_ref().expect("PtyPaste is a mutation"),
      || {
        json(crate::core::pty_remote_supervisor::paste_text(
          backend, &repo, &workspace, &label, &text, submit,
        ))
      },
    ),
    TreqCommandRequest::ReviewLoad {
      repo,
      workspace,
      scope,
    } => {
      let id = review_workspace_id(&workspace)?;
      json(load_review(&repo, id, scope))
    }
    TreqCommandRequest::ReviewSave {
      repo,
      workspace,
      scope,
      review,
    } => {
      let id = review_workspace_id(&workspace)?;
      json(save_review(&repo, id, scope, &review))
    }
    TreqCommandRequest::ReviewClear {
      repo,
      workspace,
      scope,
    } => {
      let id = review_workspace_id(&workspace)?;
      json(match scope {
        ReviewScope::Changes => crate::core::review_drafts::clear_pending_review(&repo, id),
        ReviewScope::Files => crate::core::review_drafts::clear_file_browser_review(&repo, id),
      })
    }
  }
}

pub(super) fn review_workspace_id(workspace: &String) -> Result<i64, String> {
  workspace_id(Some(workspace))?.ok_or_else(|| "invalid_arguments: workspace is required".into())
}

/// `ReviewLoad`: the host's kept review, empty when there is none.
pub(super) fn load_review(
  repo: &str,
  workspace_id: i64,
  scope: ReviewScope,
) -> Result<crate::core::review::Stored, String> {
  use crate::core::{review::Stored, review_drafts};
  Ok(match scope {
    ReviewScope::Changes => review_drafts::pending_review(repo, workspace_id)?
      .map(|review| Stored::parse(&review))
      .unwrap_or_default(),
    ReviewScope::Files => review_drafts::file_browser_review(repo, workspace_id)?
      .map(|review| Stored {
        comments: serde_json::from_str(&review.comments).unwrap_or_default(),
        summary: review.summary_text.unwrap_or_default(),
        notes: vec![],
      })
      .unwrap_or_default(),
  })
}

/// `ReviewSave`: an empty review clears what the host kept.
pub(super) fn save_review(
  repo: &str,
  workspace_id: i64,
  scope: ReviewScope,
  review: &crate::core::review::Stored,
) -> Result<(), String> {
  use crate::core::review_drafts;
  let (comments, summary, notes) = review.to_save();
  match (scope, review.is_empty()) {
    (ReviewScope::Changes, true) => review_drafts::clear_pending_review(repo, workspace_id),
    (ReviewScope::Files, true) => review_drafts::clear_file_browser_review(repo, workspace_id),
    (ReviewScope::Changes, false) => review_drafts::save_pending_review(
      repo,
      workspace_id,
      &comments,
      summary.as_deref(),
      notes.as_deref(),
    ),
    (ReviewScope::Files, false) => {
      review_drafts::save_file_browser_review(repo, workspace_id, &comments, summary.as_deref())
    }
  }
}

/// Mirrors the local `create_workspace` Tauri command: parses the same
/// metadata JSON, applies the title, and marks the workspace for its first
/// rebase. Repository settings that live in the desktop app database (copied
/// files, setup scripts) are not available on the VM and are skipped.
pub(super) fn create_workspace_from_metadata(
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
pub(super) fn ensure_working_copy_revision(
  workspace_path: &str,
  commit: &str,
) -> Result<(), String> {
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
pub(super) fn search_files_local(
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
