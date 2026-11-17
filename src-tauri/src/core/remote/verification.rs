use super::{
  execute_remote_command, DeserializeOwned, RemoteCommandError, TreqCommandRequest,
  WorkspaceChangeMarker,
};

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

pub(super) fn json_str<'a>(value: &'a serde_json::Value, field: &str) -> Option<&'a str> {
  value.get(field).and_then(|v| v.as_str())
}

/// Workspace ids are numbers in `ListWorkspaces` output but strings in typed
/// requests; compare them as text.
pub(super) fn json_id_matches(item: &serde_json::Value, id: &str) -> bool {
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
    // Archived workspaces drop out of `ListWorkspaces` just like deleted ones.
    TreqCommandRequest::DeleteWorkspace { repo, workspace }
    | TreqCommandRequest::ArchiveWorkspace { repo, workspace } => {
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
    TreqCommandRequest::SwitchRepoBranch { repo, bookmark } => {
      let bookmark = bookmark.clone();
      Some(MutationVerification {
        read_request: TreqCommandRequest::InspectRepository { repo: repo.clone() },
        check: Box::new(move |value| match value.get("current_branch") {
          Some(serde_json::Value::String(current)) if *current == bookmark => {
            MutationVerificationOutcome::AlreadyApplied
          }
          Some(_) => MutationVerificationOutcome::NotApplied,
          None => MutationVerificationOutcome::Ambiguous,
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
    // Undoing a commit abandons it too: its change id leaves the log.
    TreqCommandRequest::AbandonCommit {
      repo,
      workspace,
      commit,
      ..
    }
    | TreqCommandRequest::UndoCommit {
      repo,
      workspace,
      commit,
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
    // Discarding everything leaves no changed files to list. A nonempty
    // working copy does not prove the discard never ran: an agent or another
    // client may have written new edits after it landed, and RestoreAll has
    // no idempotency key, so rerunning it would discard those edits.
    TreqCommandRequest::RestoreAll { repo, workspace } => Some(MutationVerification {
      read_request: TreqCommandRequest::ListChanges {
        repo: repo.clone(),
        workspace: workspace.clone(),
      },
      check: Box::new(|value| match value.as_array() {
        Some(items) if items.is_empty() => MutationVerificationOutcome::AlreadyApplied,
        _ => MutationVerificationOutcome::Ambiguous,
      }),
    }),
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
pub(super) fn check_create_commit(
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
pub(super) fn check_resolve_conflict(
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

pub(super) fn change_ids_match(a: &str, b: &str) -> bool {
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
#[expect(
  clippy::expect_used,
  reason = "the request round-trips through its own JSON shape"
)]
pub(super) fn clone_request(request: &TreqCommandRequest) -> TreqCommandRequest {
  serde_json::from_value(serde_json::to_value(request).expect("TreqCommandRequest serializes"))
    .expect("TreqCommandRequest round-trips through its own JSON shape")
}
