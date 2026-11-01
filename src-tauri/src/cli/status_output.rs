use crate::core;
use crate::github::PrCiStatus;
use crate::local_db::Workspace;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

fn sidebar_activity_key(status: &core::WorkspaceSidebarStatus) -> &str {
  status
    .last_activity_at
    .as_deref()
    .filter(|value| !value.is_empty())
    .unwrap_or(status.current.created_at.as_str())
}

fn compare_sidebar_recency(
  left: &core::WorkspaceSidebarStatus,
  right: &core::WorkspaceSidebarStatus,
) -> Ordering {
  match sidebar_activity_key(right).cmp(sidebar_activity_key(left)) {
    Ordering::Equal => left.current.branch_name.cmp(&right.current.branch_name),
    other => other,
  }
}

pub(crate) fn format_workspace_metadata_lines(workspace: &Workspace, indent: &str) -> Vec<String> {
  let mut lines = vec![format!("{indent}Title: {}", workspace.title)];
  match workspace.description.as_deref() {
    Some(description) if description.contains('\n') => {
      lines.push(format!("{indent}Description:"));
      for line in description.trim_end().lines() {
        lines.push(format!("{indent}  {line}").trim_end().to_string());
      }
    }
    Some(description) => lines.push(format!("{indent}Description: {description}")),
    None => {}
  }
  lines
}

pub(crate) fn format_workspace_stack_lines(
  statuses: &[core::WorkspaceSidebarStatus],
) -> Vec<String> {
  let by_branch: HashMap<&str, &core::WorkspaceSidebarStatus> = statuses
    .iter()
    .map(|status| (status.current.branch_name.as_str(), status))
    .collect();
  let mut children: HashMap<&str, Vec<&core::WorkspaceSidebarStatus>> = HashMap::new();
  let mut roots = Vec::new();

  for status in statuses {
    match status.current.target_branch.as_deref() {
      Some(parent) if parent != status.current.branch_name && by_branch.contains_key(parent) => {
        children.entry(parent).or_default().push(status);
      }
      _ => roots.push(status),
    }
  }
  if roots.is_empty() {
    roots.extend(statuses);
  }
  roots.sort_by(|left, right| compare_sidebar_recency(left, right));
  for siblings in children.values_mut() {
    siblings.sort_by(|left, right| compare_sidebar_recency(left, right));
  }

  fn append_node(
    lines: &mut Vec<String>,
    status: &core::WorkspaceSidebarStatus,
    depth: usize,
    children: &HashMap<&str, Vec<&core::WorkspaceSidebarStatus>>,
    visited: &mut HashSet<i64>,
  ) {
    if !visited.insert(status.current.id) {
      return;
    }
    let prefix = if depth == 0 {
      String::new()
    } else {
      format!("  {}└─ ", "   ".repeat(depth - 1))
    };
    let conflict = if status.has_conflicts {
      " [CONFLICTS]"
    } else {
      ""
    };
    lines.push(format!(
      "{prefix}● {} — {}{conflict}",
      status.current.branch_name, status.current.title
    ));
    if let Some(description) = status.current.description.as_deref() {
      let label = format!("{}   Description: ", "   ".repeat(depth));
      let continuation = " ".repeat(label.chars().count());
      let body = description.trim_end_matches(['\r', '\n']);
      for (index, line) in body.lines().enumerate() {
        let lead = if index == 0 { &label } else { &continuation };
        lines.push(format!("{lead}{line}").trim_end().to_string());
      }
    }
    if let Some(descendants) = children.get(status.current.branch_name.as_str()) {
      for child in descendants {
        append_node(lines, child, depth + 1, children, visited);
      }
    }
  }

  let mut lines = vec!["Stack:".to_string()];
  let mut visited = HashSet::new();
  for root in roots {
    append_node(&mut lines, root, 0, &children, &mut visited);
  }
  lines
}

pub(crate) struct WorkspacePrStatus {
  pub github_id: String,
  pub checks: Option<PrCiStatus>,
}

pub(crate) fn format_pr_status_lines(pr: &WorkspacePrStatus, indent: &str) -> Vec<String> {
  let mut lines = vec![format!("{indent}GitHub: {}", pr.github_id)];
  if let Some(checks) = &pr.checks {
    let summary = match checks.state.as_str() {
      "success" => format!("passing ({}/{})", checks.passed, checks.total),
      "failure" => format!(
        "failing ({} failed, {} pending, {} passed)",
        checks.failed, checks.pending, checks.passed
      ),
      _ => format!(
        "pending ({} pending, {} passed, {} failed)",
        checks.pending, checks.passed, checks.failed
      ),
    };
    lines.push(format!("{indent}Checks: {summary}"));
  }
  lines
}

pub(crate) fn print_workspace_partial_status(
  status: &core::WorkspaceSidebarStatus,
  pr: Option<&WorkspacePrStatus>,
  default_branch: &str,
) {
  let flags = if status.has_conflicts {
    " [CONFLICTS]"
  } else {
    ""
  };
  println!("  ● {}{}", status.current.branch_name, flags);
  for line in format_workspace_metadata_lines(&status.current, "    ") {
    println!("{line}");
  }
  if let Some(ref target) = status.current.target_branch {
    if target != default_branch {
      println!("    Target: {}", target);
    }
  }
  if let Some(pr) = pr {
    for line in format_pr_status_lines(pr, "    ") {
      println!("{line}");
    }
  }
}

/// Keep the focused workspace, ancestor workspaces, and descendant workspaces.
/// Drop the repository default branch: it is not a workspace in the stack.
pub(crate) fn filter_statuses_for_workspace(
  statuses: &[core::WorkspaceSidebarStatus],
  branch_name: &str,
  default_branch: &str,
) -> Vec<core::WorkspaceSidebarStatus> {
  let by_branch: HashMap<&str, &core::WorkspaceSidebarStatus> = statuses
    .iter()
    .map(|status| (status.current.branch_name.as_str(), status))
    .collect();
  if !by_branch.contains_key(branch_name) {
    return Vec::new();
  }

  let mut keep: HashSet<&str> = HashSet::new();
  keep.insert(branch_name);

  let mut cursor = branch_name;
  let mut visited = HashSet::new();
  while visited.insert(cursor) {
    let Some(status) = by_branch.get(cursor) else {
      break;
    };
    let Some(parent) = status.current.target_branch.as_deref() else {
      break;
    };
    if parent == default_branch || parent == cursor || !by_branch.contains_key(parent) {
      break;
    }
    keep.insert(parent);
    cursor = parent;
  }

  let mut growing = true;
  while growing {
    growing = false;
    for status in statuses {
      let branch = status.current.branch_name.as_str();
      if keep.contains(branch) {
        continue;
      }
      let Some(parent) = status.current.target_branch.as_deref() else {
        continue;
      };
      if parent != default_branch && keep.contains(parent) {
        keep.insert(branch);
        growing = true;
      }
    }
  }

  statuses
    .iter()
    .filter(|status| keep.contains(status.current.branch_name.as_str()))
    .cloned()
    .collect()
}

pub(crate) fn format_focused_workspace_status(
  status: &core::WorkspaceStatus,
  related: &[core::WorkspaceSidebarStatus],
  uncommitted_changes: usize,
  default_branch: &str,
  pr: Option<&WorkspacePrStatus>,
) -> Vec<String> {
  let mut lines = Vec::new();
  if !related.is_empty() {
    lines.extend(format_workspace_stack_lines(related));
  }

  lines.push(format!("Workspace: {}", status.partial.current.branch_name));
  lines.extend(format_workspace_metadata_lines(
    &status.partial.current,
    "  ",
  ));
  if let Some(ref target) = status.partial.current.target_branch {
    if target != default_branch {
      lines.push(format!("  Target: {target}"));
    }
  }
  lines.push(format!("  Uncommitted changes: {uncommitted_changes}"));
  let conflict_count = status.conflicted_files.len();
  if conflict_count == 0 && !status.partial.has_conflicts {
    lines.push("  Conflicts: none".to_string());
  } else {
    let n = if conflict_count == 0 {
      1
    } else {
      conflict_count
    };
    lines.push(format!("  Conflicts: {n} files"));
    lines.push("  run `treq diff` to inspect conflicts".to_string());
  }
  lines.push(format!(
    "  Commits: {}",
    status.commits_ahead_of_target.len()
  ));
  if let Some(pr) = pr {
    lines.extend(format_pr_status_lines(pr, "  "));
  }
  lines
}

pub(crate) fn format_workspace_diff_lines(
  diff: &crate::jj::JjRevisionDiff,
  conflicted_commits: &[(String, String)],
) -> Vec<String> {
  let mut lines = Vec::new();
  if diff.conflicted_files.is_empty() && conflicted_commits.is_empty() {
    lines.push("Conflicts: none".to_string());
  } else {
    if !diff.conflicted_files.is_empty() {
      lines.push(format!(
        "Conflicted files ({})",
        diff.conflicted_files.len()
      ));
      for path in &diff.conflicted_files {
        lines.push(format!("  {path}"));
      }
    }
    if !conflicted_commits.is_empty() {
      lines.push("Conflicted commits:".to_string());
      for (change_id, description) in conflicted_commits {
        let subject = description.lines().next().unwrap_or("").trim();
        if subject.is_empty() {
          lines.push(format!("  {change_id}"));
        } else {
          lines.push(format!("  {change_id} {subject}"));
        }
      }
    }
  }

  let conflict_hunks: Vec<&crate::jj::JjFileDiff> = diff
    .hunks_by_file
    .iter()
    .filter(|file| {
      !file.conflict_regions.is_empty()
        || file
          .hunks
          .iter()
          .any(|hunk| !hunk.conflict_regions.is_empty())
        || diff.conflicted_files.iter().any(|path| path == &file.path)
    })
    .collect();
  if !conflict_hunks.is_empty() {
    lines.push("Conflict hunks:".to_string());
    for file in conflict_hunks {
      lines.push(format!("=== {} ===", file.path));
      for hunk in &file.hunks {
        if hunk.header.is_empty() && hunk.lines.is_empty() {
          continue;
        }
        lines.push(
          format!("{}\n{}", hunk.header, hunk.lines.join("\n"))
            .trim_end()
            .to_string(),
        );
      }
    }
  }
  lines
}

#[cfg(test)]
mod tests {
  use super::*;

  fn workspace(
    id: i64,
    branch: &str,
    title: &str,
    description: Option<&str>,
    target: Option<&str>,
  ) -> crate::local_db::Workspace {
    crate::local_db::Workspace {
      id,
      repo_path: "/repo".to_string(),
      workspace_name: branch.to_string(),
      workspace_path: branch.to_string(),
      branch_name: branch.to_string(),
      created_at: String::new(),
      refreshed_at: None,
      metadata: None,
      target_branch: target.map(str::to_string),
      title: title.to_string(),
      description: description.map(str::to_string),
      moved_files: None,
      not_on_remote: false,
      sparse_patterns: None,
      archived: false,
    }
  }

  fn sidebar(current: crate::local_db::Workspace) -> core::WorkspaceSidebarStatus {
    core::WorkspaceSidebarStatus {
      current,
      has_conflicts: false,
      last_activity_at: None,
    }
  }

  #[test]
  fn formats_workspace_title_and_description() {
    let current = workspace(
      1,
      "feat/auth",
      "Add authentication",
      Some("Support passkeys"),
      Some("main"),
    );

    assert_eq!(
      format_workspace_metadata_lines(&current, "  "),
      vec![
        "  Title: Add authentication",
        "  Description: Support passkeys"
      ]
    );
  }

  #[test]
  fn formats_a_multiline_description_as_an_indented_block() {
    let current = workspace(
      1,
      "feat/auth",
      "Add authentication",
      Some("## Summary\n\n- Support passkeys\n"),
      Some("main"),
    );

    assert_eq!(
      format_workspace_metadata_lines(&current, "  "),
      vec![
        "  Title: Add authentication",
        "  Description:",
        "    ## Summary",
        "",
        "    - Support passkeys",
      ]
    );
  }

  #[test]
  fn formats_workspace_stack_as_hierarchy() {
    let statuses = vec![
      sidebar(workspace(3, "feat/ui", "UI", None, Some("feat/api"))),
      sidebar(workspace(1, "main-work", "Main", None, Some("main"))),
      sidebar(workspace(2, "feat/api", "API", None, Some("main-work"))),
    ];

    assert_eq!(
      format_workspace_stack_lines(&statuses),
      vec![
        "Stack:",
        "● main-work — Main",
        "  └─ ● feat/api — API",
        "     └─ ● feat/ui — UI",
      ]
    );
  }

  #[test]
  fn stack_renders_every_line_of_a_multiline_description_aligned() {
    let statuses = vec![
      sidebar(workspace(1, "main-work", "Main", None, Some("main"))),
      sidebar(workspace(
        2,
        "feat/api",
        "API",
        Some("Add endpoints\r\n\r\n- list\n- get\n"),
        Some("main-work"),
      )),
      sidebar(workspace(3, "feat/ui", "UI", None, Some("feat/api"))),
    ];

    assert_eq!(
      format_workspace_stack_lines(&statuses),
      vec![
        "Stack:",
        "● main-work — Main",
        "  └─ ● feat/api — API",
        "      Description: Add endpoints",
        "",
        "                   - list",
        "                   - get",
        "     └─ ● feat/ui — UI",
      ]
    );
  }

  #[test]
  fn stack_renders_single_line_description_unchanged() {
    let statuses = vec![sidebar(workspace(
      1,
      "main-work",
      "Main",
      Some("Just one line\n"),
      Some("main"),
    ))];

    assert_eq!(
      format_workspace_stack_lines(&statuses),
      vec![
        "Stack:",
        "● main-work — Main",
        "   Description: Just one line"
      ]
    );
  }

  fn checks(state: &str, passed: u32, failed: u32, pending: u32) -> PrCiStatus {
    PrCiStatus {
      state: state.to_string(),
      total: passed + failed + pending,
      passed,
      failed,
      pending,
      checks: Vec::new(),
    }
  }

  #[test]
  fn formats_github_id_and_passing_checks() {
    let pr = WorkspacePrStatus {
      github_id: "acme/treq#42".to_string(),
      checks: Some(checks("success", 3, 0, 0)),
    };

    assert_eq!(
      format_pr_status_lines(&pr, "    "),
      vec!["    GitHub: acme/treq#42", "    Checks: passing (3/3)"]
    );
  }

  #[test]
  fn formats_failing_checks_with_counts() {
    let pr = WorkspacePrStatus {
      github_id: "octo/app#7".to_string(),
      checks: Some(checks("failure", 2, 1, 1)),
    };

    assert_eq!(
      format_pr_status_lines(&pr, "  "),
      vec![
        "  GitHub: octo/app#7",
        "  Checks: failing (1 failed, 1 pending, 2 passed)"
      ]
    );
  }

  #[test]
  fn omits_checks_when_github_reports_none() {
    let pr = WorkspacePrStatus {
      github_id: "octo/app#8".to_string(),
      checks: None,
    };

    assert_eq!(format_pr_status_lines(&pr, ""), vec!["GitHub: octo/app#8"]);
  }

  #[test]
  fn filter_keeps_parent_and_children_and_drops_unrelated_and_default() {
    let statuses = vec![
      sidebar(workspace(1, "feat/api", "API", None, Some("main"))),
      sidebar(workspace(2, "feat/ui", "UI", None, Some("feat/api"))),
      sidebar(workspace(3, "feat/other", "Other", None, Some("main"))),
      sidebar(workspace(4, "feat/css", "CSS", None, Some("feat/ui"))),
    ];

    let related = filter_statuses_for_workspace(&statuses, "feat/ui", "main");
    let names: Vec<&str> = related
      .iter()
      .map(|status| status.current.branch_name.as_str())
      .collect();
    assert_eq!(names, vec!["feat/api", "feat/ui", "feat/css"]);
  }

  fn focused_status(
    current: crate::local_db::Workspace,
    has_conflicts: bool,
    conflicted_files: Vec<&str>,
    commit_count: usize,
  ) -> core::WorkspaceStatus {
    core::WorkspaceStatus {
      partial: core::WorkspacePartialStatus {
        current,
        has_conflicts,
        has_changes: false,
        commits_ahead: commit_count,
      },
      conflicted_files: conflicted_files.into_iter().map(str::to_string).collect(),
      remote_sync: core::RemoteSyncStatus::InSync,
      target: None,
      children: Vec::new(),
      dag_nodes: Vec::new(),
      conflicted_workspace_ids: Vec::new(),
      commits_ahead_of_target: (0..commit_count)
        .map(|i| core::WorkspaceCommit {
          hash: format!("abcd{i:04}"),
          timestamp: String::new(),
          message: format!("commit {i}"),
        })
        .collect(),
    }
  }

  #[test]
  fn focused_status_reports_counts_and_diff_hint_without_default_branch() {
    let current = workspace(2, "feat/ui", "UI", Some("Buttons"), Some("main"));
    let status = focused_status(current.clone(), true, vec!["src/app.rs", "src/lib.rs"], 3);
    let related = vec![sidebar(current)];

    let lines = format_focused_workspace_status(&status, &related, 4, "main", None);
    assert!(lines.contains(&"Stack:".to_string()));
    assert!(lines.iter().any(|line| line.contains("feat/ui")));
    assert!(lines.contains(&"  Uncommitted changes: 4".to_string()));
    assert!(lines.contains(&"  Conflicts: 2 files".to_string()));
    assert!(lines.contains(&"  run `treq diff` to inspect conflicts".to_string()));
    assert!(lines.contains(&"  Commits: 3".to_string()));
    assert!(!lines.iter().any(|line| line.contains("Target: main")));
  }

  #[test]
  fn focused_status_omits_diff_hint_when_clean() {
    let current = workspace(1, "feat/api", "API", None, Some("feat/core"));
    let status = focused_status(current, false, vec![], 1);
    let lines = format_focused_workspace_status(&status, &[], 0, "main", None);
    assert!(lines.contains(&"  Conflicts: none".to_string()));
    assert!(!lines.iter().any(|line| line.contains("treq diff")));
    assert!(lines.contains(&"  Target: feat/core".to_string()));
  }

  #[test]
  fn diff_output_lists_conflicted_files_hunks_and_commits() {
    let diff = crate::jj::JjRevisionDiff {
      committed_files: Vec::new(),
      hunks_by_file: vec![crate::jj::JjFileDiff {
        path: "src/app.rs".to_string(),
        hunks: vec![crate::jj::JjDiffHunk {
          id: "h1".to_string(),
          header: "@@".to_string(),
          lines: ["<<<<<<< HEAD", "ours", "=======", "theirs", ">>>>>>>"]
            .map(String::from)
            .to_vec(),
          patch: String::new(),
          conflict_style: crate::conflict_markers::ConflictStyle::GitMerge,
          conflict_regions: Vec::new(),
        }],
        conflict_style: crate::conflict_markers::ConflictStyle::GitMerge,
        conflict_regions: Vec::new(),
      }],
      uncommitted_files: Vec::new(),
      conflicted_files: vec!["src/app.rs".to_string()],
      too_large_to_render: false,
      render_block_reason: None,
      working_copy_token: None,
    };

    let lines = format_workspace_diff_lines(
      &diff,
      &[("zqkxyz".to_string(), "rebase leftover\nbody".to_string())],
    );
    assert_eq!(
      lines[0..5],
      vec![
        "Conflicted files (1)",
        "  src/app.rs",
        "Conflicted commits:",
        "  zqkxyz rebase leftover",
        "Conflict hunks:",
      ]
    );
    assert!(lines.iter().any(|line| line.contains("<<<<<<< HEAD")));
  }

  #[test]
  fn diff_output_reports_none_when_clean() {
    let diff = crate::jj::JjRevisionDiff {
      committed_files: Vec::new(),
      hunks_by_file: Vec::new(),
      uncommitted_files: Vec::new(),
      conflicted_files: Vec::new(),
      too_large_to_render: false,
      render_block_reason: None,
      working_copy_token: None,
    };
    assert_eq!(
      format_workspace_diff_lines(&diff, &[]),
      vec!["Conflicts: none"]
    );
  }
}
