use crate::result_ext::ResultExt;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::jj;
use crate::local_db;

const RESOLVE_METADATA_KIND: &str = "resolve";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveSide {
  Base,
  Side1,
  Side2,
}

impl ResolveSide {
  pub fn parse(raw: &str) -> Result<Self, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
      "0" | "base" => Ok(Self::Base),
      "1" | "ours" | "left" | "side1" => Ok(Self::Side1),
      "2" | "theirs" | "right" | "side2" => Ok(Self::Side2),
      other => Err(format!(
        "Unknown conflict side '{}'. Expected 1/2/base/ours/theirs/both",
        other
      )),
    }
  }

  pub fn as_index(self) -> u8 {
    match self {
      Self::Base => 0,
      Self::Side1 => 1,
      Self::Side2 => 2,
    }
  }
}

/// Parse CLI side tokens. `both` expands to side1 then side2.
pub fn parse_resolve_sides(tokens: &[String]) -> Result<Vec<ResolveSide>, String> {
  let mut sides = Vec::new();
  for token in tokens {
    if token.eq_ignore_ascii_case("both") {
      sides.push(ResolveSide::Side1);
      sides.push(ResolveSide::Side2);
      continue;
    }
    sides.push(ResolveSide::parse(token)?);
  }
  Ok(sides)
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ResolveWorkspaceMeta {
  kind: String,
  change_id: String,
  commit_id: String,
  source_workspace_id: Option<i64>,
  workspace_slug: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ResolveTarget {
  pub change_id: String,
  pub commit_id: String,
  pub description: String,
  pub conflicted_files: Vec<String>,
  pub resolve_path: String,
  pub workspace_id: i64,
  pub workspace_name: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ResolveConflictsSession {
  pub targets: Vec<ResolveTarget>,
  /// Absolute path the agent should start in: `.treq/resolve/<workspace-slug>/`.
  pub agent_cwd: String,
  pub source_workspace_id: Option<i64>,
  pub workspace_slug: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ResolveCommitResult {
  pub success: bool,
  pub message: String,
  pub change_id: String,
  pub remaining_conflicts: Vec<String>,
}

pub fn is_resolve_workspace(workspace: &local_db::Workspace) -> bool {
  if workspace.workspace_name.starts_with("_resolve-")
    || workspace.branch_name.starts_with("treq/resolve/")
    || workspace.workspace_path.starts_with("resolve/")
  {
    return true;
  }
  workspace
    .metadata
    .as_deref()
    .and_then(|raw| serde_json::from_str::<ResolveWorkspaceMeta>(raw).ok())
    .is_some_and(|meta| meta.kind == RESOLVE_METADATA_KIND)
}

fn resolve_workspace_slug(repo_path: &str, workspace_id: Option<i64>) -> String {
  if let Some(id) = workspace_id {
    if let Ok(Some(ws)) = local_db::get_workspace_by_id(repo_path, id) {
      let slug = jj::sanitize_workspace_name(&ws.workspace_path);
      if !slug.is_empty() {
        return slug;
      }
      let from_branch = jj::sanitize_workspace_name(&ws.branch_name);
      if !from_branch.is_empty() {
        return from_branch;
      }
    }
  }
  "home".to_string()
}

fn resolve_root_path(repo_path: &str, workspace_slug: &str) -> PathBuf {
  Path::new(repo_path)
    .join(".treq")
    .join("resolve")
    .join(workspace_slug)
}

fn abs_path_for_resolve_db_workspace(repo_path: &str, workspace: &local_db::Workspace) -> String {
  // New layout stores `resolve/<slug>/<change_id>` relative to `.treq/`.
  if workspace.workspace_path.starts_with("resolve/") {
    return Path::new(repo_path)
      .join(".treq")
      .join(&workspace.workspace_path)
      .to_string_lossy()
      .to_string();
  }
  // Legacy: `_resolve-*` under `.treq/workspaces/`.
  Path::new(repo_path)
    .join(".treq")
    .join("workspaces")
    .join(&workspace.workspace_path)
    .to_string_lossy()
    .to_string()
}

fn find_existing_resolve_for_change(
  repo_path: &str,
  change_id: &str,
) -> Result<Option<local_db::Workspace>, String> {
  let all = local_db::get_workspaces(repo_path)?;
  Ok(all.into_iter().find(|w| {
    if !is_resolve_workspace(w) {
      return false;
    }
    w.metadata
      .as_deref()
      .and_then(|raw| serde_json::from_str::<ResolveWorkspaceMeta>(raw).ok())
      .is_some_and(|meta| {
        meta.kind == RESOLVE_METADATA_KIND
          && (meta.change_id == change_id
            || change_id.starts_with(&meta.change_id)
            || meta.change_id.starts_with(change_id))
      })
  }))
}

fn conflicted_files_or_empty(path: &str) -> Vec<String> {
  jj::get_conflicted_files(path, None).unwrap_or_else(|_| Vec::new())
}

fn resolve_instruction_body(workspace_slug: &str) -> String {
  format!(
    r#"# Resolve conflicts for `{slug}`

You are in a Treq resolve root: `.treq/resolve/{slug}/`.

Each subdirectory is one conflicted change id. The product workspace is untouched.

## What to do

1. Open a change-id directory and fix conflict markers in the files there.
2. Or run `treq resolve <change-id> [1|2|base|both]`.
3. Or pipe JSON path→content replacements: `echo '{{"file": "..."}}' | treq resolve <change-id>`.

`treq resolve` rewrites that commit in place. It does not create a follow-up "remove markers" commit.

## When you are done

When a change is fully resolved, Treq deletes that change-id directory.
Your work is complete when no change-id directories remain under this resolve root.
"#,
    slug = workspace_slug
  )
}

fn write_resolve_instruction_files(agent_cwd: &Path, workspace_slug: &str) -> Result<(), String> {
  fs::create_dir_all(agent_cwd).map_err(|e| {
    format!(
      "Failed to create resolve root {}: {}",
      agent_cwd.display(),
      e
    )
  })?;
  let body = resolve_instruction_body(workspace_slug);
  for name in ["AGENTS.md", "CLAUDE.md", "README.md"] {
    let path = agent_cwd.join(name);
    fs::write(&path, &body).map_err(|e| format!("Failed to write {}: {}", path.display(), e))?;
  }
  Ok(())
}

fn build_resolve_target(
  repo_path: &str,
  source_workspace_id: Option<i64>,
  workspace_slug: &str,
  commit: &jj::JjLogCommit,
) -> Result<ResolveTarget, String> {
  if let Some(existing) = find_existing_resolve_for_change(repo_path, &commit.change_id)? {
    let resolve_path = abs_path_for_resolve_db_workspace(repo_path, &existing);
    let conflicted_files = conflicted_files_or_empty(&resolve_path);
    return Ok(ResolveTarget {
      change_id: commit.change_id.clone(),
      commit_id: commit.commit_id.clone(),
      description: commit.description.clone(),
      conflicted_files,
      resolve_path,
      workspace_id: existing.id,
      workspace_name: existing.workspace_name,
    });
  }

  let (workspace_name, resolve_path) =
    jj::create_resolve_workspace(repo_path, &commit.change_id, workspace_slug)
      .map_err(|e| e.to_string())?;
  let conflicted_files = conflicted_files_or_empty(&resolve_path);

  let meta = ResolveWorkspaceMeta {
    kind: RESOLVE_METADATA_KIND.to_string(),
    change_id: commit.change_id.clone(),
    commit_id: commit.commit_id.clone(),
    source_workspace_id,
    workspace_slug: workspace_slug.to_string(),
  };
  let metadata = serde_json::to_string(&meta).unwrap_or_default();
  let branch_name = format!("treq/resolve/{}", commit.change_id);
  // Directory leaf uses the same short change id as the commit log.
  let db_path = format!("resolve/{}/{}", workspace_slug, commit.change_id);
  let workspace_id = local_db::add_workspace(
    repo_path,
    workspace_name.clone(),
    db_path,
    branch_name,
    Some(format!("Resolve {}", commit.change_id)),
    None,
    None,
  )?;
  local_db::set_workspace_metadata(repo_path, workspace_id, &metadata)?;

  Ok(ResolveTarget {
    change_id: commit.change_id.clone(),
    commit_id: commit.commit_id.clone(),
    description: commit.description.clone(),
    conflicted_files,
    resolve_path,
    workspace_id,
    workspace_name,
  })
}

/// Commits the Commits-tab inplace resolver may rewrite.
///
/// Immutable commits are included only when every conflicted commit is on the
/// workspace branch (`!on_target_only`) and `on_distinct_workspace_branch` is true.
pub fn resolvable_conflicted_commits(
  commits: &[jj::JjLogCommit],
  on_distinct_workspace_branch: bool,
) -> Vec<jj::JjLogCommit> {
  let conflicted: Vec<&jj::JjLogCommit> = commits
    .iter()
    .filter(|c| c.has_conflicts && !c.is_working_copy)
    .collect();
  let all_on_workspace_branch_only = on_distinct_workspace_branch
    && !conflicted.is_empty()
    && conflicted.iter().all(|c| !c.on_target_only);

  conflicted
    .into_iter()
    .filter(|c| !c.on_target_only && (!c.is_immutable || all_on_workspace_branch_only))
    .cloned()
    .collect()
}

fn listing_is_distinct_workspace_branch(
  repo_path: &str,
  workspace_id: Option<i64>,
) -> Result<bool, String> {
  let Some(id) = workspace_id else {
    return Ok(false);
  };
  let workspace = local_db::get_workspace_by_id(repo_path, id)
    .map_err(|e| format!("Failed to get workspace: {}", e))?
    .ok_or_else(|| format!("Workspace not found: {}", id))?;
  let default_branch = crate::jj::get_default_branch(repo_path)
    .map_err(|e| format!("Failed to resolve default branch: {}", e))?;
  Ok(workspace.branch_name != default_branch)
}

/// Start short-lived resolve workspaces for conflicted commits.
///
/// When `change_ids` is `None`, every conflicted commit in the workspace (or home)
/// history is targeted. When provided, only matching change/commit ids are used.
pub fn start_resolve_conflicts(
  repo_path: &str,
  workspace_id: Option<i64>,
  change_ids: Option<Vec<String>>,
) -> Result<ResolveConflictsSession, String> {
  let log = super::list_commits(repo_path, workspace_id, true, Some(50), None)?;
  let on_distinct = listing_is_distinct_workspace_branch(repo_path, workspace_id)?;
  let mut conflicted = resolvable_conflicted_commits(&log.commits, on_distinct);

  if let Some(ids) = change_ids {
    conflicted.retain(|c| {
      ids.iter().any(|id| {
        c.change_id.starts_with(id)
          || id.starts_with(&c.change_id)
          || c.commit_id.starts_with(id)
          || id.starts_with(&c.commit_id)
      })
    });
  }

  if conflicted.is_empty() {
    return Err(
      "No conflicted commits found to resolve. Immutable commits can only be resolved when every conflicted commit is on the workspace branch."
        .to_string(),
    );
  }

  let workspace_slug = resolve_workspace_slug(repo_path, workspace_id);
  let agent_cwd_path = resolve_root_path(repo_path, &workspace_slug);
  write_resolve_instruction_files(&agent_cwd_path, &workspace_slug)?;

  let mut targets = Vec::new();
  for commit in &conflicted {
    targets.push(build_resolve_target(
      repo_path,
      workspace_id,
      &workspace_slug,
      commit,
    )?);
  }

  Ok(ResolveConflictsSession {
    targets,
    agent_cwd: agent_cwd_path.to_string_lossy().to_string(),
    source_workspace_id: workspace_id,
    workspace_slug,
  })
}

fn find_resolve_target_by_revision(
  repo_path: &str,
  revision: &str,
) -> Result<(local_db::Workspace, String), String> {
  let all = local_db::get_workspaces(repo_path)?;
  for workspace in all {
    if !is_resolve_workspace(&workspace) {
      continue;
    }
    let meta = workspace
      .metadata
      .as_deref()
      .and_then(|raw| serde_json::from_str::<ResolveWorkspaceMeta>(raw).ok());
    let Some(meta) = meta else {
      continue;
    };
    if meta.change_id.starts_with(revision)
      || revision.starts_with(&meta.change_id)
      || meta.commit_id.starts_with(revision)
      || revision.starts_with(&meta.commit_id)
    {
      let path = abs_path_for_resolve_db_workspace(repo_path, &workspace);
      return Ok((workspace, path));
    }
  }

  Err(format!(
    "No active resolve workspace for '{}'. Start one from the Commits tab first.",
    revision
  ))
}

fn remaining_change_dirs(agent_cwd: &Path) -> Vec<PathBuf> {
  let Ok(entries) = fs::read_dir(agent_cwd) else {
    return Vec::new();
  };
  entries
    .filter_map(|entry| entry.ok())
    .map(|entry| entry.path())
    .filter(|path| path.is_dir() && path.join(".jj").exists())
    .collect()
}

fn cleanup_resolve_workspace(
  repo_path: &str,
  workspace: &local_db::Workspace,
) -> Result<(), String> {
  let full_path_str = abs_path_for_resolve_db_workspace(repo_path, workspace);
  let full_path = Path::new(&full_path_str);

  // Detach `@` from the resolved change onto a disposable empty child before
  // forgetting the resolve workspace. Otherwise `remove_wc_commit` can hide the
  // rewritten change when it was the resolve WC tip.
  if full_path.exists() {
    jj::jj_detach_resolve_workspace_wc(&full_path_str)
      .warn_on_err("jj detach resolve workspace wc");
  }

  jj::remove_workspace(repo_path, &full_path_str).warn_on_err("remove workspace");
  if workspace.id >= 0 {
    local_db::delete_workspace(repo_path, workspace.id).warn_on_err("delete workspace");
  }

  // Prefer slug from metadata; fall back to parent of the change-id dir.
  let slug = workspace
    .metadata
    .as_deref()
    .and_then(|raw| serde_json::from_str::<ResolveWorkspaceMeta>(raw).ok())
    .map(|meta| meta.workspace_slug)
    .unwrap_or_else(|| {
      full_path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("home")
        .to_string()
    });
  let agent_cwd = resolve_root_path(repo_path, &slug);
  if remaining_change_dirs(&agent_cwd).is_empty() {
    fs::remove_dir_all(&agent_cwd).best_effort();
  }

  Ok(())
}

/// Apply side picks and/or file replacements, then finalize (cleanup) when clean.
// home-head-test: resolve_workspace_conflict
pub fn resolve_commit(
  repo_path: &str,
  revision: &str,
  sides: &[ResolveSide],
  replacements: Option<HashMap<String, String>>,
) -> Result<ResolveCommitResult, String> {
  let (workspace, resolve_path) = find_resolve_target_by_revision(repo_path, revision)?;
  let had_replacements = replacements.is_some();

  if let Some(ref files) = replacements {
    jj::jj_apply_resolve_replacements(&resolve_path, files).map_err(|e| e.to_string())?;
  }

  if !sides.is_empty() {
    let indexes: Vec<u8> = sides.iter().map(|s| s.as_index()).collect();
    jj::jj_resolve_conflict_sides(&resolve_path, &indexes).map_err(|e| e.to_string())?;
  } else if !had_replacements {
    jj::jj_snapshot_resolve_workspace(&resolve_path).map_err(|e| e.to_string())?;
  }

  let remaining = conflicted_files_or_empty(&resolve_path);
  if !remaining.is_empty() {
    return Ok(ResolveCommitResult {
      success: false,
      message: format!(
        "Conflicts remain in: {}. Edit files or run `treq resolve {} 1|2|both`.",
        remaining.join(", "),
        revision
      ),
      change_id: revision.to_string(),
      remaining_conflicts: remaining,
    });
  }

  jj::jj_snapshot_resolve_workspace(&resolve_path).map_err(|e| e.to_string())?;
  cleanup_resolve_workspace(repo_path, &workspace)?;
  jj::reconcile_all_workspaces_after_rewrite(repo_path, None)
    .warn_on_err("reconcile all workspaces after rewrite");

  Ok(ResolveCommitResult {
    success: true,
    message: format!(
      "Resolved change '{}' in place and removed its resolve directory",
      revision
    ),
    change_id: revision.to_string(),
    remaining_conflicts: vec![],
  })
}

/// Build the default agent prompt listing resolve directories.
pub fn build_resolve_agent_prompt(user_prompt: &str, session: &ResolveConflictsSession) -> String {
  let mut out = String::new();
  out.push_str(
    "You are resolving jj conflicts for one or more commits without creating a new resolution commit.\n",
  );
  out.push_str(&format!(
    "Your working directory is the resolve root: {}\n",
    session.agent_cwd
  ));
  out.push_str("Read AGENTS.md / CLAUDE.md / README.md in that directory for the workflow.\n");
  out.push_str("Each subdirectory is a short-lived edit-mode sandbox for one conflicted change.\n");
  out.push_str(
    "Edit the conflict markers there, or run `treq resolve <change-id> [1|2|base|both]`.\n",
  );
  out.push_str(
    "You can also pipe a JSON object of path→content replacements: `echo '{\"file\": \"...\"}' | treq resolve <change-id>`.\n",
  );
  out.push_str(
    "When a change is fully resolved, `treq resolve` deletes that change-id directory.\n",
  );
  out.push_str(
    "Your work is complete when no change-id directories remain under the resolve root.\n\n",
  );
  out.push_str("Change directories to resolve:\n");
  for target in &session.targets {
    out.push_str(&format!(
      "- change {} ({}) → {}\n  files: {}\n  description: {}\n",
      target.change_id,
      target.commit_id,
      target.resolve_path,
      if target.conflicted_files.is_empty() {
        "(see directory)".to_string()
      } else {
        target.conflicted_files.join(", ")
      },
      target.description
    ));
  }
  if !user_prompt.trim().is_empty() {
    out.push('\n');
    out.push_str("User instructions:\n");
    out.push_str(user_prompt.trim());
    out.push('\n');
  }
  out
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::jj::JjLogCommit;

  fn commit(
    change_id: &str,
    has_conflicts: bool,
    is_immutable: bool,
    on_target_only: bool,
  ) -> JjLogCommit {
    JjLogCommit {
      commit_id: format!("commit-{change_id}"),
      short_id: change_id.to_string(),
      change_id: change_id.to_string(),
      description: change_id.to_string(),
      author_name: "Test".to_string(),
      timestamp: "2024-01-01 00:00:00".to_string(),
      parent_ids: vec![],
      is_working_copy: false,
      workspace_label: None,
      bookmarks: vec![],
      is_immutable,
      insertions: 1,
      deletions: 0,
      on_target_only,
      has_conflicts,
    }
  }

  #[test]
  fn includes_immutable_when_all_conflicted_are_workspace_only() {
    let commits = vec![commit("ws", true, true, false)];
    let resolved = resolvable_conflicted_commits(&commits, true);
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].change_id, "ws");
  }

  #[test]
  fn excludes_immutable_when_any_conflicted_is_target_only() {
    let commits = vec![
      commit("ws", true, true, false),
      commit("tgt", true, true, true),
    ];
    let resolved = resolvable_conflicted_commits(&commits, true);
    assert!(resolved.is_empty());
  }

  #[test]
  fn keeps_mutable_workspace_conflicts_when_target_is_also_conflicted() {
    let commits = vec![
      commit("ws", true, false, false),
      commit("tgt", true, true, true),
    ];
    let resolved = resolvable_conflicted_commits(&commits, true);
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].change_id, "ws");
  }

  #[test]
  fn excludes_immutable_on_home_or_default_branch() {
    let commits = vec![commit("main", true, true, false)];
    let resolved = resolvable_conflicted_commits(&commits, false);
    assert!(resolved.is_empty());
  }
}
