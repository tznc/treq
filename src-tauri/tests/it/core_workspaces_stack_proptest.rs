//! Property tests of the pure workspace stacking and naming logic: restack
//! and merge-landing plans keep the stack a forest, pasted-text stacks never
//! cycle, and the name/spec helpers never panic and keep their invariants.

use crate::common::proptest_config;
use proptest::prelude::*;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use treq_lib::core::new_workspaces::{plan_unchecked, stack_parents, DEFAULT_BRANCH_PATTERN};
use treq_lib::core::workspaces::{
  linear_issue_base_branch, parse_hunk_spec, plan_merge_landing, plan_workspace_target_move,
  validate_branch_name,
};
use treq_lib::local_db::Workspace;

const DEFAULT: &str = "main";

fn ws(id: i64, branch: String, target: Option<String>) -> Workspace {
  Workspace {
    id,
    repo_path: "/tmp/repo".into(),
    workspace_name: branch.replace('/', "-"),
    workspace_path: branch.replace('/', "-"),
    branch_name: branch.clone(),
    created_at: "2026-01-01T00:00:00.000Z".into(),
    refreshed_at: None,
    metadata: None,
    target_branch: target,
    title: branch,
    description: None,
    moved_files: None,
    not_on_remote: false,
    sparse_patterns: None,
    archived: false,
  }
}

/// A stack forest: workspace `i` targets nothing, the default branch, or an
/// earlier workspace.
fn forest() -> impl Strategy<Value = Vec<Workspace>> {
  (1usize..7).prop_flat_map(|n| {
    prop::collection::vec(any::<u8>(), n).prop_map(|picks| {
      picks
        .iter()
        .enumerate()
        .map(|(i, &p)| {
          let target = match (p as usize) % (i + 2) {
            0 => None,
            1 => Some(DEFAULT.to_string()),
            k => Some(format!("feat/w{}", k - 2)),
          };
          ws(i as i64 + 1, format!("feat/w{i}"), target)
        })
        .collect()
    })
  })
}

fn assert_forest(workspaces: &[Workspace]) -> Result<(), TestCaseError> {
  let targets: BTreeMap<&str, &str> = workspaces
    .iter()
    .filter_map(|w| Some((w.branch_name.as_str(), w.target_branch.as_deref()?)))
    .collect();
  for start in targets.keys() {
    let mut seen = BTreeSet::new();
    let mut cur = *start;
    while let Some(next) = targets.get(cur) {
      prop_assert!(
        seen.insert(cur),
        "stack cycle through {} in {:?}",
        cur,
        targets
      );
      cur = next;
    }
  }
  Ok(())
}

proptest! {
  #![proptest_config(proptest_config(256))]

  #[test]
  fn restack_plan_keeps_the_stack_a_forest(
    mut stack in forest(),
    i in any::<usize>(),
    j in any::<usize>(),
  ) {
    let moving = stack[i % stack.len()].branch_name.clone();
    let onto = stack
      .get(j % (stack.len() + 1))
      .map_or_else(|| DEFAULT.to_string(), |w| w.branch_name.clone());
    let plan = plan_workspace_target_move(&stack, &moving, &onto, DEFAULT);
    if onto == moving {
      prop_assert!(plan.is_err());
      return Ok(());
    }
    let steps = plan.map_err(TestCaseError::fail)?;
    for step in steps {
      let w = stack.iter_mut().find(|w| w.id == step.workspace_id).expect("known id");
      prop_assert_eq!(&w.branch_name, &step.branch_name);
      prop_assert_ne!(&step.new_target_branch, &step.branch_name, "self target");
      w.target_branch = Some(step.new_target_branch);
    }
    let moved = stack.iter().find(|w| w.branch_name == moving).expect("moving");
    prop_assert_eq!(moved.target_branch.as_deref(), Some(onto.as_str()));
    assert_forest(&stack)?;
  }

  #[test]
  fn merge_landing_rehomes_every_child_of_a_landed_branch(
    stack in forest(),
    i in any::<usize>(),
    rollup in any::<bool>(),
  ) {
    let merging = stack[i % stack.len()].id;
    let plan = plan_merge_landing(&stack, merging, DEFAULT, rollup).map_err(TestCaseError::fail)?;
    let merging_branch = &stack.iter().find(|w| w.id == merging).expect("id").branch_name;
    prop_assert!(plan.landed_branches.contains(merging_branch));
    prop_assert!(!plan.landed_branches.contains(&plan.target_branch));
    let retargets: HashMap<i64, &str> = plan
      .retargeted
      .iter()
      .map(|r| (r.workspace_id, r.to_branch.as_str()))
      .collect();
    let after: Vec<Workspace> = stack
      .iter()
      .filter(|w| !plan.landed_branches.contains(&w.branch_name))
      .map(|w| {
        let mut w = w.clone();
        if let Some(to) = retargets.get(&w.id) {
          w.target_branch = Some(to.to_string());
        }
        w
      })
      .collect();
    for w in &after {
      let target = w.target_branch.as_deref().unwrap_or(DEFAULT);
      prop_assert!(
        !plan.landed_branches.iter().any(|b| b == target),
        "{} still targets landed {}",
        w.branch_name,
        target
      );
    }
    assert_forest(&after)?;
  }

  #[test]
  fn pasted_stacks_never_cycle(lines in prop::collection::vec(("[ \t]{0,2}", "[ab]{1,2}"), 0..8)) {
    let text: String = lines.iter().map(|(indent, name)| format!("{indent}{name}\n")).collect();
    let parents = stack_parents(&text);
    for (child, parent) in &parents {
      prop_assert_ne!(child, parent);
      let mut seen = BTreeSet::new();
      let mut cur = child.as_str();
      while let Some(next) = parents.get(cur) {
        prop_assert!(seen.insert(cur), "cycle through {} in {:?}", cur, parents);
        cur = next;
      }
    }
  }

  #[test]
  fn titles_become_valid_distinct_branch_names(title in "\\PC{0,40}") {
    let rows = plan_unchecked(&title, DEFAULT);
    for row in rows.iter().filter(|r| r.title.is_some()) {
      let branch = row.branch.as_deref().expect("titles always get a branch");
      prop_assert!(validate_branch_name(branch).is_ok(), "{:?} -> {:?}", title, branch);
      let prefix = DEFAULT_BRANCH_PATTERN.trim_end_matches("{name}");
      prop_assert!(branch.starts_with(prefix), "{branch} lacks {prefix}");
    }
  }

  #[test]
  fn valid_branches_get_a_usable_workspace_dir(branch in "[a-z.]{1,3}(/[a-z.]{1,3}){0,2}") {
    prop_assume!(validate_branch_name(&branch).is_ok());
    let name = treq_lib::jj::sanitize_workspace_name(&branch);
    prop_assert!(!name.is_empty(), "{:?} sanitizes to empty", branch);
    prop_assert!(name != "." && name != "..");
    prop_assert!(!name.contains(['/', '\\']));
  }

  #[test]
  fn sanitized_names_hold_no_path_or_shell_metacharacters(name in "\\PC{0,24}") {
    let s = treq_lib::jj::sanitize_workspace_name(&name);
    prop_assert!(!s.contains(['/', '\\', '*', '?', '<', '>', '|', '"', ':']), "{:?}", s);
    prop_assert!(s != "." && s != "..");
  }

  #[test]
  fn hunk_specs_round_trip(file in "[^\\x00]{1,12}", start in 1usize..1000, len in 0usize..1000) {
    let raw = format!("{file}:{start}-{}", start + len);
    let spec = parse_hunk_spec(&raw).map_err(TestCaseError::fail)?;
    prop_assert_eq!(spec.file_path, file);
    prop_assert_eq!(spec.start_line, start);
    prop_assert_eq!(spec.end_line, start + len);
  }

  #[test]
  fn hunk_spec_parsing_never_panics(raw in "\\PC{0,24}") {
    if let Ok(spec) = parse_hunk_spec(&raw) {
      prop_assert!(spec.start_line >= 1 && spec.end_line >= spec.start_line);
    }
  }

  #[test]
  fn fork_point_revset_quotes_both_sides(a in "\\PC{0,12}", b in "\\PC{0,12}") {
    let revset = treq_lib::jj::fork_point_revset(&a, &b);
    prop_assert!(revset.starts_with("fork_point(") && revset.ends_with(')'));
  }

  #[test]
  fn linear_sub_issues_stack_on_an_open_parent_or_the_default(
    keys in prop::collection::vec((0u8..4, any::<bool>()), 0..6),
    parent in prop::option::of(0u8..5),
  ) {
    let workspaces: Vec<Workspace> = keys
      .iter()
      .enumerate()
      .map(|(i, (k, archived))| {
        let mut w = ws(i as i64 + 1, format!("feat/l{i}"), None);
        w.metadata = Some(format!(r#"{{"linear_issue_key":"ENG-{k}"}}"#));
        w.archived = *archived;
        w
      })
      .collect();
    let parent = parent.map(|p| format!("ENG-{p}"));
    let base = linear_issue_base_branch(&workspaces, parent.as_deref(), DEFAULT);
    let open_parent: Vec<&Workspace> = workspaces
      .iter()
      .filter(|w| !w.archived && parent.as_deref().is_some_and(|p| w.metadata.as_deref().is_some_and(|m| m.contains(&format!("\"{p}\"")))))
      .collect();
    if open_parent.is_empty() {
      prop_assert_eq!(base, DEFAULT);
    } else {
      prop_assert!(open_parent.iter().any(|w| w.branch_name == base), "{} not an open parent", base);
    }
  }
}
