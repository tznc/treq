use super::*;

/// Read the current git branch from .git/HEAD without shelling out
pub(super) fn read_git_head_branch(repo_path: &str) -> Result<String, String> {
  let head_path = Path::new(repo_path).join(".git").join("HEAD");
  let content =
    fs::read_to_string(&head_path).map_err(|e| format!("Failed to read .git/HEAD: {}", e))?;
  let trimmed = content.trim();
  if let Some(branch) = trimmed.strip_prefix("ref: refs/heads/") {
    Ok(branch.to_string())
  } else {
    Ok("HEAD".to_string())
  }
}

pub(super) fn git_local_branch_commit_hex(repo_path: &str, branch_name: &str) -> Option<String> {
  git_ref_commit_hex(repo_path, &format!("refs/heads/{branch_name}"))
}

pub(super) fn git_remote_branch_commit_hex(
  repo_path: &str,
  remote_name: &str,
  branch_name: &str,
) -> Option<String> {
  git_ref_commit_hex(
    repo_path,
    &format!("refs/remotes/{remote_name}/{branch_name}"),
  )
}

pub(super) fn git_ref_commit_hex(repo_path: &str, full_ref_name: &str) -> Option<String> {
  let repo = gix::open(repo_path).ok()?;
  let ref_name: gix::refs::FullName = full_ref_name.try_into().ok()?;
  let reference = repo.find_reference(&ref_name).ok()?;
  Some(reference.id().to_hex().to_string())
}

pub(super) fn read_origin_head_default_branch(repo_path: &str) -> Option<String> {
  let origin_head = Path::new(repo_path)
    .join(".git")
    .join("refs")
    .join("remotes")
    .join("origin")
    .join("HEAD");
  let content = fs::read_to_string(origin_head).ok()?;
  let trimmed = content.trim();
  let prefix = "ref: refs/remotes/origin/";
  Some(trimmed.strip_prefix(prefix)?.to_string())
}

/// Moves colocated Git HEAD to the home `@-` after jj rewrote history. Best effort.
///
/// Git HEAD is the home repo's source of truth: [`reconcile_colocated_home_repo`]
/// checks `@` out onto HEAD whenever the two differ, without touching disk. A
/// HEAD left on a pre-rewrite commit would therefore turn every file the
/// rewrite changed into a working-copy change.
///
/// Exports jj's bookmarks first so the branch refs are current, then attaches
/// HEAD to a branch at `@-`: the one HEAD is on, else `preferred_branch` (HEAD's
/// branch before the rewrite detached it), else the only local branch there.
/// With no such branch HEAD is detached at `@-`. The index is reset only when
/// HEAD's commit changes, so staged state survives a no-op. A HEAD that Git
/// moved since jj last imported it is left alone.
pub(super) fn sync_home_git_head_to_wc_parent(repo_path: &str, preferred_branch: Option<&str>) {
  if let Err(e) = sync_home_git_head_to_wc_parent_inner(repo_path, preferred_branch) {
    tracing::warn!("Warning: Failed to sync git HEAD to the working-copy parent: {e}");
  }
}

pub(super) fn sync_home_git_head_to_wc_parent_inner(
  repo_path: &str,
  preferred_branch: Option<&str>,
) -> Result<(), String> {
  if derive_repo_path_from_workspace(repo_path).is_some()
    || !Path::new(repo_path).join(".git").exists()
  {
    return Ok(());
  }
  let attached_branch = read_git_head_branch(repo_path)
    .ok()
    .filter(|branch| branch != "HEAD");

  let loaded = load_workspace_repo(repo_path).map_err(|e| e.to_string())?;
  let _git_lock = FileLock::lock(loaded.workspace.repo_path().join("git_import_export.lock"))
    .map_err(|e| format!("Failed to lock Git import/export: {e}"))?;
  let loaded = load_workspace_repo(repo_path).map_err(|e| e.to_string())?;
  let Some(wc_commit) = get_workspace_wc_commit(&loaded).map_err(|e| e.to_string())? else {
    return Ok(());
  };
  let [parent_id] = wc_commit.parent_ids() else {
    return Ok(());
  };
  let parent_hex = parent_id.hex();
  let wc_hex = wc_commit.id().hex();

  let head_before = gix::open(repo_path)
    .ok()
    .and_then(|repo| repo.head_id().ok().map(|id| id.to_hex().to_string()));
  let recorded_head = loaded.repo.view().git_head().as_normal().map(|id| id.hex());
  if head_before.is_some() && recorded_head.is_some() && head_before != recorded_head {
    // Git moved HEAD since jj last read it (a terminal `git switch`/`commit`).
    // That move wins; the next read reconciles `@` onto it.
    return Ok(());
  }

  let mut tx = loaded.repo.start_transaction();
  git::export_refs(tx.repo_mut()).map_err(|e| format!("Failed to export refs: {e}"))?;
  let known_branches: Vec<&str> = [attached_branch.as_deref(), preferred_branch]
    .into_iter()
    .flatten()
    .collect();
  let known_branch_at = |commit_hex: &str| {
    known_branches
      .iter()
      .find(|branch| {
        git_local_branch_commit_hex(repo_path, branch).is_some_and(|tip| tip == commit_hex)
      })
      .map(|branch| branch.to_string())
  };
  // `@` can be the branch commit itself (a working copy synced by editing the
  // bookmark). HEAD then goes on that branch; the next read starts a fresh `@`
  // on top of it without touching disk.
  let (branch, head_hex) = match known_branch_at(&parent_hex) {
    Some(branch) => (Some(branch), &parent_hex),
    None => match known_branch_at(&wc_hex) {
      Some(branch) => (Some(branch), &wc_hex),
      None => (
        find_unambiguous_local_branch_at_commit(repo_path, &parent_hex),
        &parent_hex,
      ),
    },
  };
  match &branch {
    Some(branch) => set_git_head_branch_with_gix(repo_path, branch)?,
    None if head_before.as_deref() != Some(head_hex.as_str()) => {
      set_git_head_detached_with_gix(repo_path, head_hex)?
    }
    None => {}
  }
  if head_before.as_deref() != Some(head_hex.as_str()) {
    reset_git_index_to_head_with_gix(repo_path)?;
  }

  // Record the new HEAD in the same operation, so the next read has nothing
  // to import and an Undo of the rewrite still sees it as the latest action.
  block_on(git::import_head(tx.repo_mut())).map_err(|e| format!("Failed to import HEAD: {e}"))?;
  if tx.repo().has_changes() {
    block_on(tx.commit(GIT_REFS_EXPORT_OPERATION))
      .map_err(|e| format!("Failed to commit export: {e}"))?;
  }
  Ok(())
}

pub(super) fn set_git_head_detached_with_gix(
  repo_path: &str,
  commit_hex: &str,
) -> Result<(), String> {
  let repo = gix::open(repo_path).map_err(|e| format!("Failed to open git repo with gix: {e}"))?;
  let id = gix::ObjectId::from_hex(commit_hex.as_bytes())
    .map_err(|e| format!("Invalid commit id {commit_hex}: {e}"))?;
  let head_name: gix::refs::FullName = "HEAD"
    .try_into()
    .map_err(|e| format!("Invalid HEAD ref name: {e}"))?;
  repo
    .edit_reference(RefEdit {
      change: Change::Update {
        log: Default::default(),
        expected: PreviousValue::Any,
        new: Target::Object(id),
      },
      name: head_name,
      deref: false,
    })
    .map_err(|e| format!("Failed to detach HEAD: {e}"))?;
  Ok(())
}

pub(super) fn set_git_head_branch_with_gix(repo_path: &str, branch: &str) -> Result<(), String> {
  let repo = gix::open(repo_path).map_err(|e| format!("Failed to open git repo with gix: {e}"))?;
  let target: gix::refs::FullName = format!("refs/heads/{branch}")
    .try_into()
    .map_err(|e| format!("Invalid branch ref name: {e}"))?;
  let head_name: gix::refs::FullName = "HEAD"
    .try_into()
    .map_err(|e| format!("Invalid HEAD ref name: {e}"))?;

  repo
    .edit_reference(RefEdit {
      change: Change::Update {
        log: Default::default(),
        expected: PreviousValue::Any,
        new: Target::Symbolic(target),
      },
      name: head_name,
      deref: false,
    })
    .map_err(|e| format!("Failed to update HEAD ref: {e}"))?;
  Ok(())
}

/// Finds the single local branch (refs/heads/*) whose tip commit matches `commit_hex`.
///
/// Returns `None` when no local branch matches, or when more than one does (an ambiguous
/// match is left alone rather than guessed at).
pub(super) fn find_unambiguous_local_branch_at_commit(
  repo_path: &str,
  commit_hex: &str,
) -> Option<String> {
  let repo = gix::open(repo_path).ok()?;
  let platform = repo.references().ok()?;
  let mut matches = platform
    .prefixed("refs/heads/")
    .ok()?
    .filter_map(Result::ok)
    .filter_map(|mut r| {
      let id = r.peel_to_id().ok()?;
      if id.to_hex().to_string() != commit_hex {
        return None;
      }
      r.name()
        .as_bstr()
        .to_string()
        .strip_prefix("refs/heads/")
        .map(|s| s.to_string())
    });
  let first = matches.next()?;
  if matches.next().is_some() {
    return None;
  }
  Some(first)
}

/// Reattach a detached colocated Git HEAD when it still points at a local branch tip.
///
/// jj history rewrites and ref export can leave Git's HEAD direct even though the checkout
/// is still exactly on a branch (the default branch, or the branch that was checked out
/// before an operation like commit-and-push detached HEAD at its own tip). In that state the
/// UI displays a commit hash and subsequent workspace creation can import the detached HEAD as
/// a separate line of history. A genuinely detached checkout is preserved unless its commit
/// unambiguously matches exactly one local branch, preferring the default branch when it
/// matches.
pub fn repair_detached_home_head_at_default_branch(
  repo_path: &str,
) -> Result<Option<String>, JjError> {
  if read_git_head_branch(repo_path).map_err(JjError::IoError)? != "HEAD" {
    return Ok(None);
  }

  let head_path = Path::new(repo_path).join(".git").join("HEAD");
  let detached_commit = fs::read_to_string(head_path)
    .map_err(|e| JjError::IoError(format!("Failed to read detached Git HEAD: {e}")))?;
  let detached_commit = detached_commit.trim();

  let default_branch = get_default_branch(repo_path)?;
  let matches_default = git_local_branch_commit_hex(repo_path, &default_branch)
    .is_some_and(|tip| tip == detached_commit);

  let branch = if matches_default {
    Some(default_branch)
  } else {
    find_unambiguous_local_branch_at_commit(repo_path, detached_commit)
  };

  let Some(branch) = branch else {
    return Ok(None);
  };

  set_git_head_branch_with_gix(repo_path, &branch).map_err(JjError::IoError)?;
  Ok(Some(branch))
}

pub(super) fn reset_git_index_to_head_with_gix(repo_path: &str) -> Result<(), String> {
  let repo = gix::open(repo_path).map_err(|e| format!("Failed to open git repo with gix: {e}"))?;
  let head_tree = repo
    .head_commit()
    .map_err(|e| format!("Failed to read HEAD commit: {e}"))?
    .tree_id()
    .map_err(|e| format!("Failed to read HEAD tree id: {e}"))?;
  let mut index = repo
    .index_from_tree(&head_tree)
    .map_err(|e| format!("Failed to create index from HEAD tree: {e}"))?;
  index
    .write(Default::default())
    .map_err(|e| format!("Failed to write index: {e}"))?;
  Ok(())
}

pub fn resolve_home_repo_branch(repo_path: &str) -> Result<String, JjError> {
  let git_branch = read_git_head_branch(repo_path)
    .map_err(|e| JjError::IoError(format!("Failed to determine branch: {}", e)))?;
  Ok(git_branch)
}
