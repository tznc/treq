#![expect(
  clippy::unwrap_used,
  clippy::let_underscore_must_use,
  clippy::expect_used,
  clippy::string_slice,
  reason = "test code"
)]
use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use std::fs;

#[path = "../tests/common/mod.rs"]
mod e2e_test_helpers;
use e2e_test_helpers::TestRepo;

fn create_workspace_with_target(
  repo: &TestRepo,
  branch: &str,
  target: &str,
) -> treq_lib::local_db::Workspace {
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    branch,
    Some(format!("bench workspace for {branch}")),
    None,
    Some(target),
    None,
    None,
  )
  .unwrap_or_else(|e| panic!("failed to create workspace '{branch}': {e}"));

  treq_lib::local_db::update_workspace_target_branch(&repo.repo_path, workspace.id, target)
    .unwrap_or_else(|e| panic!("failed to set target branch for '{branch}': {e}"));

  treq_lib::local_db::get_workspace_by_id(&repo.repo_path, workspace.id)
    .expect("workspace db lookup should succeed")
    .expect("workspace should exist after creation")
}

fn commit_workspace_file(
  repo: &TestRepo,
  workspace: &treq_lib::local_db::Workspace,
  file_name: &str,
  content: &str,
  message: &str,
) {
  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
  fs::write(workspace_path.join(file_name), content)
    .unwrap_or_else(|e| panic!("failed to write '{file_name}': {e}"));

  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, message)
    .unwrap_or_else(|e| panic!("failed to commit in '{}': {e}", workspace.branch_name));
}

fn setup_auto_rebase_hierarchy(repo: &TestRepo, levels: usize) -> Vec<String> {
  let mut targets = Vec::with_capacity(levels);
  let mut parent_branch = "main".to_string();

  for level in 1..=levels {
    let branch = format!("feat/bench-level-{level}");
    let workspace = create_workspace_with_target(repo, &branch, &parent_branch);

    commit_workspace_file(
      repo,
      &workspace,
      &format!("level-{level}.txt"),
      &format!("benchmark content for level {level}\n"),
      &format!("bench commit level {level}"),
    );

    targets.push(parent_branch.clone());
    parent_branch = branch;
  }

  repo
    .commit_file(
      "main-update.txt",
      "advance main so the hierarchy needs rebasing\n",
      "Advance main for auto_rebase benchmark",
    )
    .expect("failed to advance main branch");

  targets
}

fn bench_auto_rebase_two_levels(c: &mut Criterion) {
  let mut group = c.benchmark_group("auto_rebase_2_levels");
  group.sample_size(10);

  group.bench_function("hierarchy", |b| {
    b.iter_batched_ref(
      || {
        let repo = TestRepo::new().expect("failed to init test repo");
        let targets = setup_auto_rebase_hierarchy(&repo, 2);
        (repo, targets)
      },
      |(repo, targets)| {
        for target in targets {
          treq_lib::auto_rebase::rebase_workspaces_for_target(
            std::hint::black_box(&repo.repo_path),
            std::hint::black_box(target),
            std::hint::black_box("diff"),
          )
          .expect("auto_rebase failed");
        }
      },
      BatchSize::PerIteration,
    );
  });

  group.finish();
}

fn bench_auto_rebase_four_levels(c: &mut Criterion) {
  let mut group = c.benchmark_group("auto_rebase_4_levels");
  group.sample_size(10);

  group.bench_function("hierarchy", |b| {
    b.iter_batched_ref(
      || {
        let repo = TestRepo::new().expect("failed to init test repo");
        let targets = setup_auto_rebase_hierarchy(&repo, 4);
        (repo, targets)
      },
      |(repo, targets)| {
        for target in targets {
          treq_lib::auto_rebase::rebase_workspaces_for_target(
            std::hint::black_box(&repo.repo_path),
            std::hint::black_box(target),
            std::hint::black_box("diff"),
          )
          .expect("auto_rebase failed");
        }
      },
      BatchSize::PerIteration,
    );
  });

  group.finish();
}

criterion_group!(
  benches,
  bench_auto_rebase_two_levels,
  bench_auto_rebase_four_levels
);
criterion_main!(benches);
